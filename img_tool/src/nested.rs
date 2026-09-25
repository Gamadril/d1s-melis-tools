use std::fs;
use std::path::{Path, PathBuf};

const TOC1_MAGIC: u32 = 0x8911_9800;
const MINFS_MAGIC: &[u8] = b"MINFS\0";
const GPT_HEADER_ITEM_SIZE: usize = 0x4000;
/// `update.mod` erases with the raw item/leftover length. SPI NOR ioctl
/// requires that length to be a multiple of the 4 KiB sector.
const NOR_ERASE_SIZE: usize = 4096;

/// Word-wise additive checksum used by `update.mod` verify files (`V*.fex`).
fn add_sum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let words = data.len() / 4;
    for i in 0..words {
        let off = i * 4;
        sum = sum.wrapping_add(u32::from_le_bytes(data[off..off + 4].try_into().unwrap()));
    }
    // Remainder is a LE u32 with unused bytes zeroed (`fileAddSum` memcpy).
    let rem = &data[words * 4..];
    if !rem.is_empty() {
        let mut last = [0u8; 4];
        last[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_le_bytes(last));
    }
    sum
}

fn pad_nor_payload(mut data: Vec<u8>, max_size: Option<usize>, name: &str) -> Result<Vec<u8>, String> {
    let padded = data.len().div_ceil(NOR_ERASE_SIZE) * NOR_ERASE_SIZE;
    if let Some(max) = max_size {
        if padded > max {
            return Err(format!(
                "{} padded to {} bytes for 4K NOR erase, exceeds partition {} bytes",
                name, padded, max
            ));
        }
    }
    if padded != data.len() {
        data.resize(padded, 0xff);
    }
    Ok(data)
}

fn peek_header(path: &Path, n: usize) -> Result<Vec<u8>, String> {
    let data = fs::read(path).map_err(|e| format!("Failed to read {:?}: {}", path, e))?;
    Ok(data.into_iter().take(n).collect())
}

fn is_toc1(path: &Path) -> bool {
    // TOC1 header is `name[16]` then magic `0x89119800` — not at file offset 0.
    peek_header(path, 20)
        .ok()
        .filter(|h| h.len() >= 20)
        .map(|h| u32::from_le_bytes(h[16..20].try_into().unwrap()) == TOC1_MAGIC)
        .unwrap_or(false)
}

fn is_minfs(path: &Path) -> bool {
    peek_header(path, MINFS_MAGIC.len())
        .ok()
        .map(|h| h.as_slice() == MINFS_MAGIC)
        .unwrap_or(false)
}

fn item_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("Failed to read {:?}: {}", dir, e))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.eq_ignore_ascii_case("image.cfg") {
            continue;
        }
        files.push(path);
    }
    files.sort();
    Ok(files)
}

/// After a flat IMAGEWTY unpack, extract TOC1 (bootA) and MinFS (ROOTFS)
/// payloads into `<item>.out/`, same idea as `dump_tool extract`.
pub fn extract_nested_items(dir: &Path, verbose: bool) -> Result<(), String> {
    for path in item_files(dir)? {
        let meta = fs::metadata(&path).map_err(|e| format!("Failed to stat {:?}: {}", path, e))?;
        if meta.len() < 32 {
            continue;
        }
        let out_dir = PathBuf::from(format!("{}.out", path.display()));
        if is_toc1(&path) {
            if verbose {
                println!("  Extracting bootA (TOC1) from: {}", path.display());
            } else {
                println!("Extracting boot package {:?} to {:?}", path, out_dir);
            }
            melis_boot::extract(&path, &out_dir)?;
        } else if is_minfs(&path) {
            if verbose {
                println!("  Extracting ROOTFS (MinFS) from: {}", path.display());
            } else {
                println!("Extracting MINFS {:?} to {:?}", path, out_dir);
            }
            minfs::extract(&path, &out_dir)?;
        }
    }
    Ok(())
}

fn replace_item_file(item: &Path, packed: &Path) -> Result<(), String> {
    fs::rename(packed, item)
        .or_else(|_| {
            fs::copy(packed, item)
                .map(|_| ())
                .and_then(|_| fs::remove_file(packed))
        })
        .map_err(|e| format!("Failed to replace {:?}: {}", item, e))
}

fn update_verify_file(
    dir: &Path,
    item_name: &str,
    payload: &[u8],
    verbose: bool,
) -> Result<(), String> {
    let verify_name = format!("V{}", item_name);
    let verify_path = dir.join(&verify_name);
    if !verify_path.exists() {
        return Ok(());
    }
    let sum = add_sum(payload);
    if verbose {
        println!(
            "  Updating verify file {} (add_sum=0x{:08x})",
            verify_name, sum
        );
    }
    fs::write(&verify_path, sum.to_le_bytes())
        .map_err(|e| format!("Failed to write {:?}: {}", verify_path, e))
}

/// Pack `<item>.out/` trees back into the IMAGEWTY payload files, then
/// refresh sibling `V<item>` checksums if present (`update.mod` verify).
pub fn pack_nested_items(dir: &Path, verbose: bool) -> Result<(), String> {
    for path in item_files(dir)? {
        let out_dir = PathBuf::from(format!("{}.out", path.display()));
        if !out_dir.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let packed = PathBuf::from(format!("{}.repacked", path.display()));

        if out_dir.join("melis-config.bin").is_file() || is_toc1(&path) {
            if verbose {
                println!("  Repacking bootA package: {}", path.display());
            } else {
                println!("Repacking bootA {:?} -> {:?}", path, packed);
            }
            let config_path = out_dir.join("melis-config.bin");
            melis_boot::pack(&path, &config_path, &packed)?;
        } else if is_minfs(&path) || out_dir.join("apps").is_dir() {
            if verbose {
                println!("  Packing MINFS directory: {}", out_dir.display());
            } else {
                println!("Packing MINFS directory {:?} to {:?}", out_dir, packed);
            }
            minfs::pack(&out_dir, &packed)?;
        } else {
            continue;
        }

        let payload =
            fs::read(&packed).map_err(|e| format!("Failed to read packed {:?}: {}", packed, e))?;
        let payload = pad_nor_payload(payload, None, &name)?;
        fs::write(&packed, &payload)
            .map_err(|e| format!("Failed to write padded {:?}: {}", packed, e))?;
        replace_item_file(&path, &packed)?;
        update_verify_file(dir, &name, &payload, verbose)?;
    }
    Ok(())
}

/// Phoenix `update_mbr` `get_file_name`: basename, `.`→`_`, uppercased, 16 chars.
/// Buffer is pre-filled with ASCII `'0'` like the host tool.
fn phoenix_item_name(path_or_name: &str) -> [u8; 16] {
    let basename = path_or_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path_or_name);
    let mut out = [b'0'; 16];
    let mut i = 0;
    for c in basename.chars() {
        if i >= 16 {
            break;
        }
        out[i] = if c == '.' {
            b'_'
        } else {
            c.to_ascii_uppercase() as u8
        };
        i += 1;
    }
    out
}

fn subtype_str(bytes: &[u8; 16]) -> String {
    let len = bytes.iter().position(|&c| c == 0).unwrap_or(16);
    String::from_utf8_lossy(&bytes[..len]).into_owned()
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

const SUNXI_MBR_SIZE: usize = 16 * 1024;
const SUNXI_MBR_COPIES: usize = 4;
const SUNXI_PART_SIZE: usize = 128;
const SUNXI_DL_PART_SIZE: usize = 72;

fn pad16(s: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let bytes = s.as_bytes();
    let n = bytes.len().min(16);
    out[..n].copy_from_slice(&bytes[..n]);
    out
}

fn download_filename_for(part_name: &str) -> Option<&'static str> {
    match part_name {
        "bootA" => Some("melis_pkg_nor.fex"),
        "ROOTFS" => Some("data_udisk.fex"),
        _ => None,
    }
}

/// this dump's GPT geometry instead of `sys_partition_nor.fex` sizes.
fn build_sunxi_mbr_and_dlinfo(parts: &[gpt::PartitionInfo]) -> (Vec<u8>, Vec<u8>) {
    let mut mbr = vec![0u8; SUNXI_MBR_SIZE];
    mbr[4..8].copy_from_slice(&0x0000_0200u32.to_le_bytes());
    mbr[8..16].copy_from_slice(b"softw411");
    mbr[16..20].copy_from_slice(&(SUNXI_MBR_COPIES as u32).to_le_bytes());
    mbr[24..28].copy_from_slice(&(parts.len() as u32).to_le_bytes());

    let mut dl = vec![0u8; SUNXI_MBR_SIZE];
    dl[4..8].copy_from_slice(&0x0000_0200u32.to_le_bytes());
    dl[8..16].copy_from_slice(b"softw411");

    let mut down_index = 0usize;
    for (i, p) in parts.iter().enumerate() {
        let len_sectors = (p.size / 512) as u32;
        let off = 32 + i * SUNXI_PART_SIZE;
        mbr[off..off + 4].copy_from_slice(&((p.start_lba >> 32) as u32).to_le_bytes());
        mbr[off + 4..off + 8].copy_from_slice(&(p.start_lba as u32).to_le_bytes());
        mbr[off + 12..off + 16].copy_from_slice(&len_sectors.to_le_bytes());
        mbr[off + 16..off + 32].copy_from_slice(&pad16("DISK"));
        mbr[off + 32..off + 48].copy_from_slice(&pad16(&p.name));
        mbr[off + 48..off + 52].copy_from_slice(&0x8000u32.to_le_bytes());

        if let Some(dl_name) = download_filename_for(&p.name) {
            let fname = phoenix_item_name(dl_name);
            let mut vf = [b'0'; 16];
            vf[0] = b'V';
            vf[1..].copy_from_slice(&fname[..15]);

            let doff = 32 + down_index * SUNXI_DL_PART_SIZE;
            dl[doff..doff + 16].copy_from_slice(&pad16(&p.name));
            dl[doff + 16..doff + 20].copy_from_slice(&((p.start_lba >> 32) as u32).to_le_bytes());
            dl[doff + 20..doff + 24].copy_from_slice(&(p.start_lba as u32).to_le_bytes());
            dl[doff + 28..doff + 32].copy_from_slice(&len_sectors.to_le_bytes());
            dl[doff + 32..doff + 48].copy_from_slice(&fname);
            dl[doff + 48..doff + 64].copy_from_slice(&vf);
            dl[doff + 68..doff + 72].copy_from_slice(&1u32.to_le_bytes());
            down_index += 1;
        }
    }
    dl[16..20].copy_from_slice(&(down_index as u32).to_le_bytes());

    let mut mbr_file = Vec::with_capacity(SUNXI_MBR_SIZE * SUNXI_MBR_COPIES);
    for index in 0..SUNXI_MBR_COPIES {
        mbr[20..24].copy_from_slice(&(index as u32).to_le_bytes());
        let crc = crc32_ieee(&mbr[4..]);
        mbr[0..4].copy_from_slice(&crc.to_le_bytes());
        mbr_file.extend_from_slice(&mbr);
    }

    let dl_crc = crc32_ieee(&dl[4..]);
    dl[0..4].copy_from_slice(&dl_crc.to_le_bytes());
    (mbr_file, dl)
}

fn write_image_cfg(dir: &Path) -> Result<(), String> {
    let mut cfg = String::new();
    cfg.push_str(";/**************************************************************************/\n");
    cfg.push_str("; generated by melis_tools img_tool from-dump\n");
    cfg.push_str(
        ";/**************************************************************************/\n\n",
    );
    cfg.push_str("[DIR_DEF]\n");
    cfg.push_str("INPUT_DIR = \"./\"\n\n");
    cfg.push_str("[FILELIST]\n");
    cfg.push_str(
        "\t{filename = \"boot0_nor.fex\", maintype = \"12345678\", subtype = \"1234567890BNOR_0\",},\n",
    );
    cfg.push_str(
        "\t{filename = \"sunxi_gpt.fex\", maintype = \"12345678\", subtype = \"1234567890___GPT\",},\n",
    );
    cfg.push_str(
        "\t{filename = \"sunxi_mbr_nor.fex\", maintype = \"12345678\", subtype = \"1234567890___MBR\",},\n",
    );
    cfg.push_str(
        "\t{filename = \"dlinfo.fex\", maintype = \"12345678\", subtype = \"1234567890DLINFO\",},\n",
    );
    let boota = subtype_str(&phoenix_item_name("melis_pkg_nor.fex"));
    let vboota = subtype_str(&phoenix_item_name("Vmelis_pkg_nor.fex"));
    let rootfs = subtype_str(&phoenix_item_name("data_udisk.fex"));
    let vrootfs = subtype_str(&phoenix_item_name("Vdata_udisk.fex"));
    cfg.push_str(&format!(
        "\t{{filename = \"melis_pkg_nor.fex\", maintype = \"RFSFAT16\", subtype = \"{}\",}},\n",
        boota
    ));
    cfg.push_str(&format!(
        "\t{{filename = \"Vmelis_pkg_nor.fex\", maintype = \"RFSFAT16\", subtype = \"{}\",}},\n",
        vboota
    ));
    cfg.push_str(&format!(
        "\t{{filename = \"data_udisk.fex\", maintype = \"RFSFAT16\", subtype = \"{}\",}},\n",
        rootfs
    ));
    cfg.push_str(&format!(
        "\t{{filename = \"Vdata_udisk.fex\", maintype = \"RFSFAT16\", subtype = \"{}\",}},\n",
        vrootfs
    ));
    cfg.push_str("\n[IMAGE_CFG]\n");
    cfg.push_str("version = 0x100234\n");
    cfg.push_str("pid = 0x1234\n");
    cfg.push_str("vid = 0x8743\n");
    cfg.push_str("hardwareid = 0x100\n");
    cfg.push_str("firmwareid = 0x100\n");
    cfg.push_str("filelist = FILELIST\n");
    fs::write(dir.join("image.cfg"), cfg).map_err(|e| format!("Failed to write image.cfg: {}", e))
}

/// Build a Phoenix IMAGEWTY `.img` from a `dump_tool extract` directory.
///
/// `update.mod` opens payloads by maintype/subtype. Mapping matches vendor
/// `image.cfg` / `sys_partition_nor.fex`: boot0 (`BNOR_0`), GPT header,
/// bootA (`melis_pkg_nor.fex`), ROOTFS (`data_udisk.fex` — SDK name).
pub fn pack_dump_dir_as_image(
    dump_dir: &Path,
    output_img: &Path,
    verbose: bool,
) -> Result<(), String> {
    let packed = dump::pack_firmware(dump_dir, verbose)?;

    let stage = dump_dir.join(".img_pack_stage");
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|e| format!("Failed to clear staging dir: {}", e))?;
    }
    fs::create_dir_all(&stage).map_err(|e| format!("Failed to create staging dir: {}", e))?;

    let write = |name: &str, data: &[u8]| -> Result<(), String> {
        fs::write(stage.join(name), data).map_err(|e| format!("Failed to write {}: {}", name, e))
    };

    let parts = gpt::read_partition_table(dump_dir.join("gpt.bin"))?;
    let (mbr, dlinfo) = build_sunxi_mbr_and_dlinfo(&parts);
    let part_size = |name: &str| -> Option<usize> {
        parts
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.size as usize)
    };
    let boota = pad_nor_payload(packed.boota, part_size("bootA"), "bootA")?;
    let rootfs = pad_nor_payload(packed.rootfs, part_size("ROOTFS"), "ROOTFS")?;
    if verbose {
        println!(
            "  IMAGEWTY payloads padded to 4K NOR erase: bootA {} bytes, ROOTFS {} bytes",
            boota.len(),
            rootfs.len()
        );
    }

    write("boot0_nor.fex", &packed.boot0)?;
    let gpt_len = packed.gpt.len().min(GPT_HEADER_ITEM_SIZE);
    write("sunxi_gpt.fex", &packed.gpt[..gpt_len])?;
    write("sunxi_mbr_nor.fex", &mbr)?;
    write("dlinfo.fex", &dlinfo)?;
    write("melis_pkg_nor.fex", &boota)?;
    write("Vmelis_pkg_nor.fex", &add_sum(&boota).to_le_bytes())?;
    write("data_udisk.fex", &rootfs)?;
    write("Vdata_udisk.fex", &add_sum(&rootfs).to_le_bytes())?;
    write_image_cfg(&stage)?;

    if verbose {
        println!("  Packing IMAGEWTY from dump tree via {}", stage.display());
    }
    let pack_result = image::pack_image(&stage, output_img);
    let _ = fs::remove_dir_all(&stage);
    pack_result
}

pub fn is_dump_extract_dir(dir: &Path) -> bool {
    dir.join("boot0.bin").is_file() && dir.join("gpt.bin").is_file()
}

pub fn is_img_extract_dir(dir: &Path) -> bool {
    dir.join("image.cfg").is_file()
}
