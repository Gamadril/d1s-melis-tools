use std::alloc::Layout;
use std::fs::File;
use std::io::SeekFrom;
use std::mem::size_of;
use std::path::{Path, PathBuf};

use lzma_rust2::LzmaReader;

use binrw::io::BufReader;
use binrw::io::Cursor;
use binrw::io::Read;
use binrw::io::Seek;
use binrw::io::Write;
use binrw::{BinRead, BinWrite};

use crate::structs::*;

pub(crate) fn extract_fs(
    image_path: impl AsRef<Path>,
    dest_path: impl AsRef<Path>,
) -> Result<(), String> {
    let in_file = File::open(image_path).map_err(|e| format!("Error opening input file: {}", e))?;
    let in_file_size = in_file.metadata().unwrap().len();
    let mut reader = BufReader::new(in_file);

    let minfs_header =
        MinfsHeader::read(&mut reader).map_err(|e| format!("Error reading input file: {}", e))?;
    let magic = String::from_utf8(minfs_header.magic).unwrap();
    if magic != MINFS_MAGIC {
        return Err(format!("Input file is not a MINFS image"));
    }

    if in_file_size < minfs_header.size.into() {
        return Err(format!(
            "Input file size ({} bytes) is smaller than the MINFS header info ({} bytes)",
            in_file_size, minfs_header.size
        ));
    }

    let root_entry = MinfsDirEntry {
        offset: minfs_header.root_dir_offset,
        size: minfs_header.root_dir_size,
        record_len: 20,
        unpack_size: 0,
        attribute: MINFS_ATTR_DIR,
        //name_len: 0,
        //extent_len: 0,
        name: vec![],
        extent: vec![],
    };

    let out_dir = PathBuf::from(dest_path.as_ref());

    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)
            .map_err(|e| format!("Error deleting output directory: {}", e))?;
    }
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Error creating output directory: {}", e))?;

    extract_dir(&root_entry, &out_dir, &mut reader)?;
    println!();

    Ok(())
}

fn progress_dot() {
    print!(".");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

fn extract_file(
    dir_entry: &MinfsDirEntry,
    dest_path: impl AsRef<Path>,
    reader: &mut BufReader<File>,
) -> Result<(), String> {
    let name = String::from_utf8_lossy(&dir_entry.name).into_owned();
    let mut path = PathBuf::from(dest_path.as_ref());
    path.push(&name);

    let mut file =
        File::create(path).map_err(|e| format!("Error creating file {}: {}", &name, e))?;

    reader
        .seek(std::io::SeekFrom::Start(dir_entry.offset as u64))
        .map_err(|e| format!("Error seek: {}", e))?;
    let mut buffer = vec![0u8; dir_entry.size as usize];
    reader
        .read_exact(&mut buffer)
        .map_err(|e| format!("Error reading entry: {}", e))?;

    if dir_entry.attribute & MINFS_ATTR_MODULE == MINFS_ATTR_MODULE {
        let out: Vec<u8> = Vec::with_capacity(dir_entry.unpack_size as usize);
        let mut out_writer = Cursor::new(out);

        let elf_header = Elf32Ehdr {
            e_ident: Elf32Ident {
                ei_mag0: EI_MAG0,
                ei_mag1: EI_MAG1,
                ei_mag2: EI_MAG2,
                ei_mag3: EI_MAG3,
                ei_class: ELFCLASS32,
                ei_data: ELFDATA2LSB,
                ei_version: EV_CURRENT,
                ei_osabi: ELFOSABI_NONE,
                ei_abiversion: 0,
                ei_pad0: 0,
                ei_pad1: 0,
                ei_pad2: 0,
                ei_pad3: 0,
                ei_pad4: 0,
                ei_pad5: 0,
                ei_nident: 0,
            },
            e_type: ET_EXEC,
            e_machine: EM_RISCV,
            e_version: 1,
            e_entry: 0,
            e_phoff: size_of::<Elf32Ehdr>() as u32,
            e_shoff: size_of::<Elf32Ehdr>() as u32 + size_of::<Elf32Phdr>() as u32,
            e_flags: 5,
            e_ehsize: size_of::<Elf32Ehdr>() as u16,
            e_phentsize: size_of::<Elf32Phdr>() as u16,
            e_phnum: 1,
            e_shentsize: size_of::<Elf32Shdr>() as u16,
            e_shnum: dir_entry.extent.len() as u16 + 2,
            e_shstrndx: dir_entry.extent.len() as u16 + 1,
        };
        elf_header.write(&mut out_writer).unwrap();

        let p_offset = Layout::from_size_align(
            size_of::<Elf32Ehdr>()
                + size_of::<Elf32Phdr>()
                + (elf_header.e_shnum as usize) * size_of::<Elf32Shdr>(),
            MINFS_DATA_ALIGN as usize,
        )
        .unwrap();
        let mut sh_offset = p_offset.clone();
        let total_size = dir_entry
            .extent
            .iter()
            .map(|var| var.record_unpack_size)
            .reduce(|a, b| a + b)
            .unwrap();
        let total_mem = dir_entry
            .extent
            .iter()
            .map(|var| var.size)
            .reduce(|a, b| a + b)
            .unwrap();

        let prog_header = Elf32Phdr {
            p_type: PT_LOAD,
            p_offset: p_offset.size() as u32,
            p_vaddr: dir_entry.extent[0].virtual_address,
            p_paddr: dir_entry.extent[0].virtual_address,
            p_filesz: total_size,
            p_memsz: total_mem,
            p_flags: PF_R | PF_W | PF_X,
            p_align: MINFS_DATA_ALIGN,
        };
        prog_header.write(&mut out_writer).unwrap();

        let mut section_headers: Vec<Elf32Shdr> = Vec::with_capacity(dir_entry.extent.len());

        let section_header_empty = Elf32Shdr {
            sh_name: 0,
            sh_type: SectionType::SHT_NULL,
            sh_flags: 0,
            sh_addr: 0,
            sh_offset: 0,
            sh_size: 0,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 0,
            sh_entsize: 0,
        };
        section_headers.push(section_header_empty);

        for mfs_sec in &dir_entry.extent {
            let mut name_len = match mfs_sec.flags {
                0x6 => DOTTEXT_OFFSET,
                0x2 => DOTRODATA_OFFSET,
                0x3 => match mfs_sec.record_size {
                    0 => DOTBSS_OFFSET,
                    _ => DOTDATA_OFFSET,
                },
                _ => 0,
            };

            if mfs_sec.attribute & MINFS_SECTION_ATTR_MAGIC == MINFS_SECTION_ATTR_MAGIC {
                name_len = MAGIC_OFFSET;
            }

            let section_header = Elf32Shdr {
                sh_name: name_len as u32,
                sh_type: mfs_sec.section_type,
                sh_flags: mfs_sec.flags,
                sh_addr: mfs_sec.virtual_address,
                sh_offset: sh_offset.size() as u32,
                sh_size: if mfs_sec.record_unpack_size > 0 {
                    mfs_sec.record_unpack_size
                } else {
                    mfs_sec.size
                },
                sh_link: 0,
                sh_info: 0,
                sh_addralign: MINFS_DATA_ALIGN,
                sh_entsize: 0,
            };
            sh_offset = Layout::from_size_align(
                sh_offset.size() + mfs_sec.record_unpack_size as usize,
                MINFS_DATA_ALIGN as usize,
            )
            .unwrap();

            if mfs_sec.record_unpack_size == 0 && mfs_sec.record_size == 0 {
                // Do not write anything, but keep the section header!
            } else if &mfs_sec.attribute & MINFS_SECTION_ATTR_COMPRESS
                == MINFS_SECTION_ATTR_COMPRESS
            {
                out_writer
                    .seek(SeekFrom::Start(section_header.sh_offset as u64))
                    .unwrap();
                let start = mfs_sec.offset as usize;
                let end = start + mfs_sec.record_size as usize;
                let sec_buffer = &buffer[start..end];
                let mut sec_reader = Cursor::new(sec_buffer);
                let probs = LzmaProbs::read_le(&mut sec_reader).unwrap();
                progress_dot();

                let mut lzma_reader = LzmaReader::new_with_props(
                    sec_reader,
                    mfs_sec.record_unpack_size as u64,
                    probs.props,
                    probs.dict_size,
                    None,
                )
                .unwrap();
                let mut buf = vec![0; mfs_sec.record_unpack_size as usize];
                lzma_reader.read_exact(&mut buf).unwrap();

                out_writer.write_all(&mut buf).unwrap();
            } else if mfs_sec.record_size > 0 {
                out_writer
                    .seek(SeekFrom::Start(section_header.sh_offset as u64))
                    .unwrap();
                let start = mfs_sec.offset as usize;
                let end = start + mfs_sec.record_size as usize;
                let sec_buffer = &buffer[start..end];

                out_writer
                    .write_all(sec_buffer)
                    .map_err(|e| format!("Error writing to file {}: {}", &name, e))?;
            }
            section_headers.push(section_header);
        }

        let str_1 = format!("{}\0", MINFS_DEFAULT_SECTION_NAME);
        let str_2 = format!("{}\0", MINFS_MODULE_MAGIC);

        let str_table = vec![
            str_1.as_bytes(),
            ".text\0".as_bytes(),
            ".rodata\0".as_bytes(),
            ".data\0".as_bytes(),
            ".bss\0".as_bytes(),
            ".shstrtab\0".as_bytes(),
            str_2.as_bytes(),
        ];
        let str_table_size: usize = str_table.iter().map(|&s| s.len()).sum();

        let section_header = Elf32Shdr {
            sh_name: DOTSHSTRTAB_OFFSET as u32,
            sh_type: SectionType::SHT_STRTAB,
            sh_flags: 0,
            sh_addr: 0,
            sh_offset: sh_offset.size() as u32,
            sh_size: str_table_size as u32,
            sh_link: 0,
            sh_info: 0,
            sh_addralign: 0,
            sh_entsize: 0,
        };
        section_headers.push(section_header);

        out_writer
            .seek(SeekFrom::Start(elf_header.e_shoff as u64))
            .unwrap();
        for sh in &section_headers {
            sh.write(&mut out_writer).unwrap();
        }

        out_writer
            .seek(SeekFrom::Start(
                (prog_header.p_offset + prog_header.p_filesz) as u64,
            ))
            .unwrap();
        str_table.iter().for_each(|&s| {
            out_writer.write(s).unwrap();
        });

        // Update the ELF header in the cursor before writing to file
        let actual_shnum = section_headers.len() as u16;
        let actual_shstrndx = (section_headers.len() - 1) as u16;
        let mut final_elf_header = elf_header;
        final_elf_header.e_shnum = actual_shnum;
        final_elf_header.e_shstrndx = actual_shstrndx;

        out_writer.seek(SeekFrom::Start(0)).unwrap();
        final_elf_header.write(&mut out_writer).unwrap();

        file.write_all(out_writer.get_ref())
            .map_err(|e: std::io::Error| format!("Error writing to file {}: {}", &name, e))?;
    } else if dir_entry.attribute & MINFS_ATTR_COMPRESS == MINFS_ATTR_COMPRESS {
        let mut out: Vec<u8> = Vec::with_capacity(dir_entry.unpack_size as usize);
        progress_dot();

        let mut sec_reader = Cursor::new(buffer);
        let probs = LzmaProbs::read_le(&mut sec_reader).unwrap();

        let mut lzma_reader = LzmaReader::new_with_props(
            sec_reader,
            dir_entry.unpack_size as u64,
            probs.props,
            probs.dict_size,
            None,
        )
        .map_err(|e| format!("Error creating lzma reader: {}", e))?;
        lzma_reader
            .read_exact(&mut out)
            .map_err(|e| format!("Error extracting lzma data: {}", e))?;

        file.write_all(&out)
            .map_err(|e: std::io::Error| format!("Error writing to file {}: {}", &name, e))?;
    } else {
        file.write_all(&buffer)
            .map_err(|e| format!("Error writing to file {}: {}", &name, e))?;
    }

    Ok(())
}

fn extract_dir(
    dir_entry: &MinfsDirEntry,
    dest_path: impl AsRef<Path>,
    reader: &mut BufReader<File>,
) -> Result<(), String> {
    let mut dir_path = PathBuf::from(dest_path.as_ref());

    //if dir_entry.name_len > 0 {
    let dir_name = String::from_utf8_lossy(&dir_entry.name).into_owned();
    dir_path.push(&dir_name);
    if !dir_path.exists() {
        std::fs::create_dir(&dir_path)
            .map_err(|e| format!("Error creating directory {}: {}", &dir_name, e))?;
    }
    //}

    reader
        .seek(std::io::SeekFrom::Start(dir_entry.offset as u64))
        .map_err(|e| format!("Error seek: {}", e))?;
    let pos = reader.stream_position().unwrap();
    while reader.stream_position().unwrap() < pos + dir_entry.size as u64 {
        let entry =
            MinfsDirEntry::read(reader).map_err(|e| format!("Error reading entry: {}", e))?;
        let ret_pos = reader.stream_position().unwrap();
        if entry.attribute & MINFS_ATTR_DIR == MINFS_ATTR_DIR {
            extract_dir(&entry, &dir_path, reader)?;
        } else {
            extract_file(&entry, &dir_path, reader)?;
        }
        reader
            .seek(std::io::SeekFrom::Start(ret_pos))
            .map_err(|e| format!("Error seek: {}", e))?;
    }

    Ok(())
}
