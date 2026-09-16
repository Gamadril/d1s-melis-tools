use std::{
    fs::{self, File},
    io::{self, BufWriter, Cursor, Read, Seek, Write},
    path::{Path, PathBuf},
};

use binrw::{BinRead, BinWrite};
use lzma_rust2::{LzmaOptions, LzmaWriter};

use crate::structs::*;

fn read_null_terminated_string_from_buffer(buffer: &[u8], start: usize) -> Option<String> {
    if start >= buffer.len() {
        return None;
    }

    let end = buffer[start..].iter().position(|&c| c == 0)?;
    let slice = &buffer[start..start + end];

    match std::str::from_utf8(slice) {
        Ok(valid_str) => Some(valid_str.to_string()),
        Err(_) => None,
    }
}

#[derive(Clone)]
struct ImageNode {
    is_dir: bool,
    path: PathBuf,
    name: Vec<u8>,
    attribute: u16,
    unpack_size: u32,
    size: u32,
    offset: u32,
    extent: Vec<MinfsSectionHeader>,
    file_data: Vec<u8>,
    dentry_offset: u32,
    dentry_size: u32,
    children: Vec<usize>,
}

fn collect_nodes(dir_path: &Path, nodes: &mut Vec<ImageNode>) -> Result<Vec<usize>, String> {
    let mut dir_entries: Vec<PathBuf> = std::fs::read_dir(dir_path)
        .map_err(|e| format!("Error reading directory {:?}: {}", dir_path, e))?
        .filter_map(|r| r.ok().map(|e| e.path()))
        .collect();

    // Sort alphabetically, case-insensitive, for deterministic generation
    dir_entries.sort_by(|a, b| {
        a.file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
            .cmp(&b.file_name().unwrap().to_string_lossy().to_lowercase())
    });

    let mut children_indices = Vec::new();

    for entry in &dir_entries {
        let is_dir = entry.is_dir();
        let name = entry
            .file_name()
            .unwrap()
            .to_string_lossy()
            .as_bytes()
            .to_vec();

        let node_idx = nodes.len();
        nodes.push(ImageNode {
            is_dir,
            path: entry.clone(),
            name,
            attribute: 0,
            unpack_size: 0,
            size: 0,
            offset: 0,
            extent: Vec::new(),
            file_data: Vec::new(),
            dentry_offset: 0,
            dentry_size: 0,
            children: Vec::new(),
        });
        children_indices.push(node_idx);
    }

    // Recursively collect children for subdirectories
    for &idx in &children_indices {
        if nodes[idx].is_dir {
            let sub_path = nodes[idx].path.clone();
            let sub_children = collect_nodes(&sub_path, nodes)?;
            nodes[idx].children = sub_children;
        }
    }

    Ok(children_indices)
}

fn valid_module_section(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let filter = [".note", ".comment", ".shstrtab"];
    for f in &filter {
        if name.eq_ignore_ascii_case(f) {
            return false;
        }
    }
    true
}

fn parse_elf_module(
    raw_data: &[u8],
    compress_on: bool,
) -> Result<(Vec<MinfsSectionHeader>, Vec<u8>), String> {
    let mut reader = Cursor::new(raw_data);
    let elf_header =
        Elf32Ehdr::read(&mut reader).map_err(|e| format!("Failed to read ELF header: {}", e))?;

    // Check ELF magic bytes
    if elf_header.e_ident.ei_mag0 != 0x7F
        || elf_header.e_ident.ei_mag1 != b'E'
        || elf_header.e_ident.ei_mag2 != b'L'
        || elf_header.e_ident.ei_mag3 != b'F'
    {
        return Err("Invalid ELF magic".to_string());
    }

    // Read all section headers
    let mut sheaders = Vec::new();
    reader
        .seek(io::SeekFrom::Start(elf_header.e_shoff as u64))
        .map_err(|e| format!("Failed to seek to section headers: {}", e))?;
    for _ in 0..elf_header.e_shnum {
        let shdr = Elf32Shdr::read(&mut reader)
            .map_err(|e| format!("Failed to read section header: {}", e))?;
        sheaders.push(shdr);
    }

    // Read shstrtab data
    let shstrtab_sec = &sheaders[elf_header.e_shstrndx as usize];
    let mut shstrtab = vec![0; shstrtab_sec.sh_size as usize];
    reader
        .seek(io::SeekFrom::Start(shstrtab_sec.sh_offset as u64))
        .map_err(|e| format!("Failed to seek to shstrtab: {}", e))?;
    reader
        .read_exact(&mut shstrtab)
        .map_err(|e| format!("Failed to read shstrtab: {}", e))?;

    let mut mfs_sections = Vec::new();
    let mut file_data = Vec::new();
    let mut current_offset = 0;

    for shdr in &sheaders {
        if shdr.sh_type == SectionType::SHT_NULL {
            continue;
        }

        let name = read_null_terminated_string_from_buffer(&shstrtab, shdr.sh_name as usize)
            .unwrap_or_default();

        if !valid_module_section(&name) {
            continue;
        }

        let mut attribute = 0;
        if name == MINFS_MODULE_MAGIC {
            attribute |= MINFS_SECTION_ATTR_MAGIC;
        }

        let record_size;
        let record_unpack_size;
        let offset;

        if shdr.sh_type == SectionType::SHT_NOBITS {
            offset = 0;
            record_size = 0;
            record_unpack_size = 0;
        } else {
            let mut sec_data = vec![0; shdr.sh_size as usize];
            reader
                .seek(io::SeekFrom::Start(shdr.sh_offset as u64))
                .map_err(|e| format!("Failed to seek to section data: {}", e))?;
            reader
                .read_exact(&mut sec_data)
                .map_err(|e| format!("Failed to read section data: {}", e))?;

            let mut final_sec_data = sec_data;
            record_unpack_size = shdr.sh_size;

            if compress_on
                && name != MINFS_MODULE_MAGIC
                && shdr.sh_size > MINFS_SECTION_COMRPESS_MIN
            {
                let mut lzma_option = LzmaOptions::with_preset(5);
                lzma_option.dict_size = 32 * 1024;

                let mut compressed = Vec::new();
                let mut comp_writer = Cursor::new(&mut compressed);

                let lzma_probs = LzmaProbs {
                    props: lzma_option.get_props(),
                    dict_size: lzma_option.dict_size,
                };
                lzma_probs
                    .write(&mut comp_writer)
                    .map_err(|e| format!("Failed to write LZMA probs: {}", e))?;

                let mut lzma_writer =
                    LzmaWriter::new_no_header(&mut comp_writer, &lzma_option, false)
                        .map_err(|e| format!("Failed to create LZMA writer: {}", e))?;

                lzma_writer
                    .write_all(&final_sec_data)
                    .map_err(|e| format!("Failed to write to LZMA encoder: {}", e))?;
                lzma_writer
                    .finish()
                    .map_err(|e| format!("Failed to finish LZMA encoder: {}", e))?;

                final_sec_data = compressed;
                attribute |= MINFS_SECTION_ATTR_COMPRESS;
            }

            offset = current_offset;
            record_size = final_sec_data.len() as u32;

            file_data.extend_from_slice(&final_sec_data);
            let pad_len = minfs_align(record_size, MINFS_DATA_ALIGN) - record_size;
            file_data.extend(std::iter::repeat(0).take(pad_len as usize));

            current_offset += minfs_align(record_size, MINFS_DATA_ALIGN);
        }

        mfs_sections.push(MinfsSectionHeader {
            offset,
            record_size,
            record_unpack_size,
            size: shdr.sh_size,
            virtual_address: shdr.sh_addr,
            section_type: shdr.sh_type,
            flags: shdr.sh_flags,
            attribute,
        });
    }

    Ok((mfs_sections, file_data))
}

fn compute_dentry_size(name_len: usize, extent_bytes: usize) -> u32 {
    let aligned_name_len = minfs_align(name_len as u32, MINFS_NAME_ALIGN as u32);
    let raw_size = 20 + aligned_name_len + (extent_bytes as u32);
    minfs_align(raw_size, MINFS_DENTRY_ALIGN as u32)
}

fn layout_dentries(node_idx: usize, current_offset: &mut u32, nodes: &mut [ImageNode]) {
    let children_indices = nodes[node_idx].children.clone();
    if children_indices.is_empty() {
        return;
    }

    let children_start = *current_offset;
    let mut total_size = 0;

    for &child_idx in &children_indices {
        nodes[child_idx].dentry_offset = *current_offset;
        *current_offset += nodes[child_idx].dentry_size;
        total_size += nodes[child_idx].dentry_size;
    }

    nodes[node_idx].offset = children_start;
    nodes[node_idx].size = total_size;

    for &child_idx in &children_indices {
        if nodes[child_idx].is_dir {
            layout_dentries(child_idx, current_offset, nodes);
        }
    }
}

pub(crate) fn build_fs(
    folder_path: impl AsRef<Path>,
    image_path: impl AsRef<Path>,
) -> Result<(), String> {
    let folder_path = folder_path.as_ref();
    let image_path = image_path.as_ref();

    if !folder_path.exists() {
        return Err(format!("Input folder {:?} does not exist", folder_path));
    }

    let mut nodes = Vec::new();
    let root_children_indices = collect_nodes(folder_path, &mut nodes)?;

    // Process nodes to determine sizes and compress files
    for node in &mut nodes {
        if node.is_dir {
            node.attribute = MINFS_ATTR_DIR;
            node.dentry_size = compute_dentry_size(node.name.len(), 0);
        } else {
            let raw_data = fs::read(&node.path)
                .map_err(|e| format!("Failed to read file {:?}: {}", node.path, e))?;
            node.unpack_size = raw_data.len() as u32;

            let ext = node
                .path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let compress_on = COMPRESS_EXT.contains(&ext.as_str());

            let is_elf = raw_data.len() >= 4 && &raw_data[0..4] == &[0x7F, b'E', b'L', b'F'];
            let is_normal_elf = ext == "elf";

            if is_elf && !is_normal_elf {
                let (extent, file_data) = parse_elf_module(&raw_data, compress_on)?;
                node.extent = extent;
                node.file_data = file_data;
                node.attribute = MINFS_ATTR_MODULE;
                if compress_on {
                    node.attribute |= MINFS_ATTR_COMPRESS;
                }
                node.size = node.file_data.len() as u32;
                node.dentry_size = compute_dentry_size(node.name.len(), node.extent.len() * 32);
            } else {
                node.attribute = 0;
                if compress_on {
                    let mut lzma_option = LzmaOptions::with_preset(5);
                    lzma_option.dict_size = 32 * 1024;

                    let mut compressed = Vec::new();
                    let mut comp_writer = Cursor::new(&mut compressed);

                    let lzma_probs = LzmaProbs {
                        props: lzma_option.get_props(),
                        dict_size: lzma_option.dict_size,
                    };
                    lzma_probs
                        .write(&mut comp_writer)
                        .map_err(|e| format!("Failed to write LZMA probs: {}", e))?;

                    let mut lzma_writer =
                        LzmaWriter::new_no_header(&mut comp_writer, &lzma_option, false)
                            .map_err(|e| format!("Failed to create LZMA writer: {}", e))?;

                    lzma_writer
                        .write_all(&raw_data)
                        .map_err(|e| format!("Failed to write data to LZMA encoder: {}", e))?;
                    lzma_writer
                        .finish()
                        .map_err(|e| format!("Failed to finish LZMA encoder: {}", e))?;

                    node.file_data = compressed;
                    node.attribute |= MINFS_ATTR_COMPRESS;
                } else {
                    node.file_data = raw_data;
                }
                node.size = node.file_data.len() as u32;
                node.dentry_size = compute_dentry_size(node.name.len(), 0);
            }
        }
    }

    // Add dummy root node
    let root_idx = nodes.len();
    nodes.push(ImageNode {
        is_dir: true,
        path: PathBuf::new(),
        name: Vec::new(),
        attribute: MINFS_ATTR_DIR,
        unpack_size: 0,
        size: 0,
        offset: 0,
        extent: Vec::new(),
        file_data: Vec::new(),
        dentry_offset: 0,
        dentry_size: 0,
        children: root_children_indices,
    });

    // Layout directory entries starting at MINFS_HEADER_LEN
    let mut current_offset = MINFS_HEADER_LEN;
    layout_dentries(root_idx, &mut current_offset, &mut nodes);
    let final_dentry_offset = current_offset;

    // Layout file data aligned to sector size
    let fdata_offset = minfs_align(final_dentry_offset, MINFS_SECTOR_LEN);
    let mut current_data_offset = fdata_offset;

    for idx in 0..root_idx {
        if !nodes[idx].is_dir {
            nodes[idx].offset = current_data_offset;
            current_data_offset += minfs_align(nodes[idx].size, MINFS_DATA_ALIGN);
        }
    }
    let final_data_offset = current_data_offset;

    // Prepare files and write to target image
    let out_file =
        File::create(image_path).map_err(|e| format!("Error creating output image file: {}", e))?;
    let mut writer = BufWriter::new(out_file);

    // Write placeholder header
    let placeholder_header = MinfsHeader {
        magic: MINFS_MAGIC.as_bytes().to_vec(),
        version: MINFS_VERSION,
        root_dir_offset: 0,
        root_dir_size: 0,
        dentry_num: 0,
        dentry_len: 0,
        fdata_len: 0,
        size: 0,
        reserved: vec![0; 480],
    };
    placeholder_header
        .write(&mut writer)
        .map_err(|e| format!("Failed to write placeholder header: {}", e))?;

    // Sort actual nodes by dentry_offset to write them sequentially
    let mut actual_nodes = nodes[0..root_idx].to_vec();
    actual_nodes.sort_by_key(|n| n.dentry_offset);

    // Write dentries
    for node in &actual_nodes {
        let dentry = MinfsDirEntry {
            offset: node.offset,
            size: node.size,
            unpack_size: node.unpack_size,
            record_len: node.dentry_size as u16,
            attribute: node.attribute,
            name: node.name.clone(),
            extent: node.extent.clone(),
        };
        writer
            .seek(io::SeekFrom::Start(node.dentry_offset as u64))
            .map_err(|e| format!("Failed to seek to dentry offset: {}", e))?;
        dentry
            .write(&mut writer)
            .map_err(|e| format!("Failed to write dentry: {}", e))?;
    }

    // Write file data
    writer
        .seek(io::SeekFrom::Start(fdata_offset as u64))
        .map_err(|e| format!("Failed to seek to file data offset: {}", e))?;
    for node in &actual_nodes {
        if !node.is_dir {
            writer
                .seek(io::SeekFrom::Start(node.offset as u64))
                .map_err(|e| format!("Failed to seek to file offset: {}", e))?;
            writer
                .write_all(&node.file_data)
                .map_err(|e| format!("Failed to write file data: {}", e))?;

            let aligned_size = minfs_align(node.size, MINFS_DATA_ALIGN);
            let pad_len = aligned_size - node.size;
            if pad_len > 0 {
                writer
                    .write_all(&vec![0; pad_len as usize])
                    .map_err(|e| format!("Failed to write file data padding: {}", e))?;
            }
        }
    }

    // Align final image size to sector boundary
    let final_pos = writer
        .stream_position()
        .map_err(|e| format!("Failed to get stream position: {}", e))? as u32;
    let aligned_final_pos = minfs_align(final_pos, MINFS_SECTOR_LEN);
    if aligned_final_pos > final_pos {
        writer
            .write_all(&vec![0; (aligned_final_pos - final_pos) as usize])
            .map_err(|e| format!("Failed to write final sector padding: {}", e))?;
    }

    // Seek back to 0 and write the finalized, correct MinfsHeader
    let final_header = MinfsHeader {
        magic: MINFS_MAGIC.as_bytes().to_vec(),
        version: MINFS_VERSION,
        root_dir_offset: nodes[root_idx].offset,
        root_dir_size: nodes[root_idx].size,
        dentry_num: actual_nodes.len() as u32,
        dentry_len: final_dentry_offset - MINFS_HEADER_LEN,
        fdata_len: final_data_offset - fdata_offset,
        size: aligned_final_pos,
        reserved: vec![0; 480],
    };
    writer
        .seek(io::SeekFrom::Start(0))
        .map_err(|e| format!("Failed to seek to start for final header: {}", e))?;
    final_header
        .write(&mut writer)
        .map_err(|e| format!("Failed to write final header: {}", e))?;

    writer
        .flush()
        .map_err(|e| format!("Failed to flush image writer: {}", e))?;

    Ok(())
}
