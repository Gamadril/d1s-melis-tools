use binrw::BinRead;
use std::convert::TryInto;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

pub mod structs;
use structs::{Toc1ItemInfo, Toc1MainInfo};

pub mod fex_compiler;
pub use fex_compiler::compile_sys_config;

use lzma_rust::LZMAReader;

const TOC1_MAGIC: u32 = 0x8911_9800;
const TOC1_CHECKSUM_STAMP: u32 = 0x5F0A_6C39;

fn calc_toc1_checksum(data: &[u8]) -> u32 {
    let mut buf = data.to_vec();
    buf[20..24].copy_from_slice(&TOC1_CHECKSUM_STAMP.to_le_bytes());

    let words: Vec<u32> = buf
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();

    let mut sum: u32 = 0;
    let mut count = words.len();
    let mut idx = 0;
    while count > 3 {
        sum = sum.wrapping_add(words[idx]);
        sum = sum.wrapping_add(words[idx + 1]);
        sum = sum.wrapping_add(words[idx + 2]);
        sum = sum.wrapping_add(words[idx + 3]);
        idx += 4;
        count -= 4;
    }
    while count > 0 {
        sum = sum.wrapping_add(words[idx]);
        idx += 1;
        count -= 1;
    }
    sum
}

fn read_toc1_items(data: &[u8]) -> Result<(Toc1MainInfo, Vec<Toc1ItemInfo>), String> {
    let mut cursor = Cursor::new(data);
    let main_info = Toc1MainInfo::read(&mut cursor)
        .map_err(|e| format!("Failed to parse TOC1 main info: {}", e))?;
    if main_info.magic != TOC1_MAGIC {
        return Err(format!("Invalid TOC1 magic: {:#x}", main_info.magic));
    }

    let mut items = Vec::new();
    for _ in 0..main_info.num_items {
        let item = Toc1ItemInfo::read(&mut cursor)
            .map_err(|e| format!("Failed to parse TOC1 item info: {}", e))?;
        items.push(item);
    }
    Ok((main_info, items))
}

/// Repack a bootA image, replacing the melis-config payload and refreshing TOC1 checksum.
pub fn pack(
    template_boot_path: impl AsRef<Path>,
    melis_config_path: impl AsRef<Path>,
    output_boot_path: impl AsRef<Path>,
) -> Result<(), String> {
    let mut boot = std::fs::read(template_boot_path.as_ref())
        .map_err(|e| format!("Failed to read boot template: {}", e))?;
    let config = std::fs::read(melis_config_path.as_ref())
        .map_err(|e| format!("Failed to read melis-config.bin: {}", e))?;

    let (main_info, items) = read_toc1_items(&boot)?;
    let valid_len = main_info.length as usize;
    if valid_len > boot.len() {
        return Err(format!(
            "TOC1 valid_len ({}) exceeds boot image size ({})",
            valid_len,
            boot.len()
        ));
    }

    let mut config_item = None;
    for item in &items {
        let name = String::from_utf8_lossy(&item.name)
            .trim_end_matches('\0')
            .to_string();
        if name == "melis-config" {
            config_item = Some(item.clone());
            break;
        }
    }
    let config_item = config_item.ok_or("melis-config item not found in boot template")?;

    if config.len() > config_item.length as usize {
        return Err(format!(
            "melis-config payload ({} bytes) exceeds slot size ({} bytes)",
            config.len(),
            config_item.length
        ));
    }

    let start = config_item.offset as usize;
    let end = start + config_item.length as usize;
    if end > boot.len() {
        return Err("melis-config slot out of bounds in boot template".to_string());
    }

    boot[start..start + config.len()].copy_from_slice(&config);
    if config.len() < config_item.length as usize {
        boot[start + config.len()..end].fill(0);
    }

    let checksum = calc_toc1_checksum(&boot[..valid_len]);
    boot[20..24].copy_from_slice(&checksum.to_le_bytes());

    std::fs::write(output_boot_path.as_ref(), &boot)
        .map_err(|e| format!("Failed to write repacked boot image: {}", e))?;

    println!(
        "Repacked bootA: melis-config {} bytes, TOC1 checksum 0x{:08x}, valid_len 0x{:x}",
        config.len(),
        checksum,
        valid_len
    );
    Ok(())
}

fn decompress_lzma(compressed: &[u8]) -> Result<Vec<u8>, String> {
    if compressed.len() >= 14 {
        println!(
            "melis-boot LZMA payload b = {:02X}, code = {:08X}",
            compressed[13],
            u32::from_be_bytes(compressed[14..18].try_into().unwrap())
        );
    }
    let mut reader = std::io::Cursor::new(compressed);
    let mut lzma_reader = LZMAReader::new_mem_limit(&mut reader, u32::MAX, None)
        .map_err(|e| format!("LZMA decoder initialization failed: {}", e))?;

    let mut decompressed = Vec::new();
    lzma_reader
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("LZMA decompression failed: {}", e))?;

    Ok(decompressed)
}

pub fn decompile_sys_config(data: &[u8]) -> Result<String, String> {
    if data.len() < 16 {
        return Err("Configuration data too short".to_string());
    }

    let item_num = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let _v0 = u32::from_le_bytes(data[4..8].try_into().unwrap());
    let _v1 = u32::from_le_bytes(data[8..12].try_into().unwrap());
    let _v2 = u32::from_le_bytes(data[12..16].try_into().unwrap());

    let mut out = String::new();
    out.push_str(";/****** decompiled from sys_config.bin ******/\n\n");

    let mut offset = 16;
    let mut groups = Vec::new();
    for _ in 0..item_num {
        if offset + 40 > data.len() {
            return Err("Unexpected EOF reading groups".to_string());
        }
        let grp_name_bytes = &data[offset..offset + 32];
        let grp_len = u32::from_le_bytes(data[offset + 32..offset + 36].try_into().unwrap());
        let grp_offset = u32::from_le_bytes(data[offset + 36..offset + 40].try_into().unwrap());
        offset += 40;

        let grp_name = String::from_utf8_lossy(grp_name_bytes)
            .trim_end_matches('\0')
            .to_string();
        groups.push((grp_name, grp_len, grp_offset));
    }

    for (grp_name, grp_len, grp_offset) in groups {
        out.push_str(&format!("[{}]\n", grp_name));
        let grp_byte_offset = (grp_offset * 4) as usize;

        for i in 0..grp_len {
            let subkey_offset = grp_byte_offset + (i as usize) * 40;
            if subkey_offset + 40 > data.len() {
                return Err("Unexpected EOF reading subkeys".to_string());
            }
            let sub_name_bytes = &data[subkey_offset..subkey_offset + 32];
            let val_word_offset = u32::from_le_bytes(
                data[subkey_offset + 32..subkey_offset + 36]
                    .try_into()
                    .unwrap(),
            );
            let type_info = u32::from_le_bytes(
                data[subkey_offset + 36..subkey_offset + 40]
                    .try_into()
                    .unwrap(),
            );

            let sub_name = String::from_utf8_lossy(sub_name_bytes)
                .trim_end_matches('\0')
                .to_string();

            let val_byte_offset = (val_word_offset * 4) as usize;
            let word_len = (type_info & 0xFFFF) as usize;
            let val_type = ((type_info >> 16) & 0xFFFF) as usize;

            if val_byte_offset + word_len * 4 > data.len() {
                return Err(format!(
                    "Value offset out of bounds for subkey {}",
                    sub_name
                ));
            }

            if val_type == 1 {
                let val_int = i32::from_le_bytes(
                    data[val_byte_offset..val_byte_offset + 4]
                        .try_into()
                        .unwrap(),
                );
                out.push_str(&format!("{} = {}\n", sub_name, val_int));
            } else if val_type == 2 {
                let str_bytes = &data[val_byte_offset..val_byte_offset + word_len * 4];
                let str_val = String::from_utf8_lossy(str_bytes)
                    .trim_end_matches('\0')
                    .to_string();
                out.push_str(&format!("{} = \"{}\"\n", sub_name, str_val));
            } else if val_type == 4 {
                if word_len < 6 {
                    return Err(format!(
                        "GPIO type requires at least 6 words for subkey {}",
                        sub_name
                    ));
                }
                let mut words = Vec::new();
                for w in 0..6 {
                    let bo = val_byte_offset + w * 4;
                    words.push(i32::from_le_bytes(data[bo..bo + 4].try_into().unwrap()));
                }
                let port = words[0];
                let pin = words[1];
                let mux = words[2];
                let pull = words[3];
                let drv = words[4];
                let data_val = words[5];

                let port_str = if port == 0xffff {
                    "POWER".to_string()
                } else {
                    format!("P{}", (b'A' + port as u8 - 1) as char)
                };

                let fmt = |v: i32| -> String {
                    if v == -1 {
                        "default".to_string()
                    } else {
                        v.to_string()
                    }
                };

                out.push_str(&format!(
                    "{} = port:{}{:02}<{}><{}><{}><{}>\n",
                    sub_name,
                    port_str,
                    pin,
                    fmt(mux),
                    fmt(pull),
                    fmt(drv),
                    fmt(data_val)
                ));
            } else if val_type == 5 {
                // DATA_EMPTY: `key =` with nothing after it. Emit it back
                // the same way, so recompiling with fex_compiler produces
                // the same DATA_EMPTY entry instead of a bogus string.
                out.push_str(&format!("{} =\n", sub_name));
            } else {
                let mut words = Vec::new();
                for w in 0..word_len {
                    let bo = val_byte_offset + w * 4;
                    words.push(i32::from_le_bytes(data[bo..bo + 4].try_into().unwrap()));
                }
                out.push_str(&format!(
                    "{} = ; Type {}, len {}, words: {:?}\n",
                    sub_name, val_type, word_len, words
                ));
            }
        }
        out.push('\n');
    }

    Ok(out)
}

fn generate_pin_mappings_md(fex_str: &str) -> String {
    let mut out = String::new();
    out.push_str("# GPIO Pin Mappings\n\n");

    let mut current_group = String::new();
    let mut group_has_table = false;

    for line in fex_str.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_group = line[1..line.len() - 1].to_string();
            group_has_table = false;
        } else if line.contains(" = port:") {
            if let Some(parts) = line.split_once(" = ") {
                let key = parts.0.trim();
                let val = parts.1.trim();

                if val.starts_with("port:") {
                    let pin_spec = &val[5..]; // e.g. PF00<2><1><default><default>
                    if let Some(bracket_idx) = pin_spec.find('<') {
                        let pin_name = &pin_spec[..bracket_idx];
                        let params_str = &pin_spec[bracket_idx..]; // <2><1><default><default>

                        let params: Vec<&str> = params_str
                            .split('>')
                            .map(|s| s.trim_start_matches('<'))
                            .filter(|s| !s.is_empty())
                            .collect();

                        if params.len() >= 4 {
                            if !group_has_table {
                                out.push_str(&format!("## `{}`\n", current_group));
                                out.push_str("| Key Name | Pin | Mux Mode | Pull Resistor | Drive Strength | Initial Output |\n");
                                out.push_str("| :--- | :--- | :--- | :--- | :--- | :--- |\n");
                                group_has_table = true;
                            }

                            let mux = match params[0] {
                                "0" => "`0` (GPIO Input)".to_string(),
                                "1" => "`1` (GPIO Output)".to_string(),
                                "default" => "`default`".to_string(),
                                other => format!("`{}` (Alt Function)", other),
                            };

                            let pull = match params[1] {
                                "0" => "`0` (Float)".to_string(),
                                "1" => "`1` (Pull-Up)".to_string(),
                                "2" => "`2` (Pull-Down)".to_string(),
                                "default" => "`default`".to_string(),
                                other => format!("`{}`", other),
                            };

                            let drv = match params[2] {
                                "0" => "`0` (Level 0 / 10mA)".to_string(),
                                "1" => "`1` (Level 1 / 20mA)".to_string(),
                                "2" => "`2` (Level 2 / 30mA)".to_string(),
                                "3" => "`3` (Level 3 / 40mA)".to_string(),
                                "default" => "`default`".to_string(),
                                other => format!("`{}`", other),
                            };

                            let data = match params[3] {
                                "0" => "`0` (Low Output)".to_string(),
                                "1" => "`1` (High Output)".to_string(),
                                "default" => "`default`".to_string(),
                                other => format!("`{}`", other),
                            };

                            out.push_str(&format!(
                                "| `{}` | **{}** | {} | {} | {} | {} |\n",
                                key, pin_name, mux, pull, drv, data
                            ));
                        }
                    }
                }
            }
        }
    }
    out
}

fn analyze_kernel(data: &[u8]) {
    println!("\n=== Melis Kernel Analysis ===");

    // Check for common headers and magic bytes
    if data.len() < 512 {
        println!("  Warning: Kernel too small ({} bytes)", data.len());
        return;
    }

    // Look for ELF header
    if data.len() >= 4 && &data[0..4] == b"\x7fELF" {
        println!("  Format: ELF executable");
        if data.len() >= 5 {
            println!(
                "  Class: {}",
                if data[4] == 1 { "32-bit" } else { "64-bit" }
            );
        }
        if data.len() >= 6 {
            println!(
                "  Endianness: {}",
                if data[5] == 1 { "Little" } else { "Big" }
            );
        }
        if data.len() >= 18 {
            let machine = u16::from_le_bytes([data[18], data[19]]);
            let arch = match machine {
                0x28 => "ARM",
                0xB7 => "AArch64",
                0xF3 => "RISC-V",
                _ => "Unknown",
            };
            println!("  Architecture: {} (0x{:04x})", arch, machine);
        }
    } else {
        println!("  Format: Raw binary (no ELF header)");
    }

    // Search for common strings
    let data_str = String::from_utf8_lossy(data);
    let keywords = [
        ("Melis", "Melis OS"),
        ("epos", "EPOS RTOS"),
        ("RTOS", "Real-Time OS"),
        ("FreeRTOS", "FreeRTOS"),
        ("RT-Thread", "RT-Thread"),
        ("sunxi", "Allwinner SoC"),
        ("D1s", "Allwinner D1s"),
        ("RISC-V", "RISC-V arch"),
    ];

    println!("  Detected strings:");
    for (keyword, desc) in &keywords {
        if data_str.contains(keyword) {
            println!("    - {} detected", desc);
        }
    }

    // Look for version strings
    if let Some(ver_idx) = data_str.find("version") {
        let snippet = &data_str[ver_idx..ver_idx.min(ver_idx + 100).min(data_str.len())];
        if let Some(line) = snippet.lines().next() {
            println!("  Version info: {}", line.trim());
        }
    }

    // Check entry point patterns (first few instructions for RISC-V)
    if data.len() >= 16 {
        println!("  Entry point (first 16 bytes): {:02X?}", &data[0..16]);
    }

    println!(
        "  Total size: {} KB ({} bytes)\n",
        data.len() / 1024,
        data.len()
    );
}

pub fn extract(boot_path: impl AsRef<Path>, dest_dir: impl AsRef<Path>) -> Result<(), String> {
    let boot_path = boot_path.as_ref();
    let dest_dir = dest_dir.as_ref();
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("Failed to create destination directory: {}", e))?;

    let mut file =
        File::open(boot_path).map_err(|e| format!("Failed to open boot image: {}", e))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|e| format!("Failed to read boot image: {}", e))?;

    let mut cursor = Cursor::new(&data);
    let main_info = Toc1MainInfo::read(&mut cursor)
        .map_err(|e| format!("Failed to parse TOC1 main info: {}", e))?;

    // Verify magic
    if main_info.magic != 0x89119800 {
        return Err(format!("Invalid TOC1 magic: {:#x}", main_info.magic));
    }

    let mut items = Vec::new();
    for _ in 0..main_info.num_items {
        let item = Toc1ItemInfo::read(&mut cursor)
            .map_err(|e| format!("Failed to parse TOC1 item info: {}", e))?;
        items.push(item);
    }

    for item in items {
        let name = String::from_utf8_lossy(&item.name)
            .trim_end_matches('\0')
            .to_string();

        let start = item.offset as usize;
        let end = start + item.length as usize;
        if end > data.len() {
            return Err(format!("Item {} offset/length out of bounds", name));
        }

        let payload = &data[start..end];

        // 1. Write the raw file
        let raw_filename = format!("{}.bin", name);
        let raw_path = dest_dir.join(&raw_filename);
        let mut out_file = File::create(&raw_path)
            .map_err(|e| format!("Failed to create output file {}: {}", raw_filename, e))?;
        out_file
            .write_all(payload)
            .map_err(|e| format!("Failed to write output file {}: {}", raw_filename, e))?;

        // 2. If name contains lzma, try to decompress
        if name.contains("lzma") {
            println!("Decompressing LZMA item: {}", name);
            match decompress_lzma(payload) {
                Ok(decompressed) => {
                    println!(
                        "  Compressed size: {} bytes, Decompressed size: {} bytes, Ratio: {:.2}x",
                        payload.len(),
                        decompressed.len(),
                        decompressed.len() as f64 / payload.len() as f64
                    );

                    let decomp_filename = format!("{}.decompressed", name);
                    let decomp_path = dest_dir.join(&decomp_filename);
                    let mut decomp_file = File::create(&decomp_path).map_err(|e| {
                        format!(
                            "Failed to create decompressed file {}: {}",
                            decomp_filename, e
                        )
                    })?;
                    decomp_file.write_all(&decompressed).map_err(|e| {
                        format!(
                            "Failed to write decompressed file {}: {}",
                            decomp_filename, e
                        )
                    })?;

                    if name.contains("melis") {
                        let epos_path = dest_dir.join("epos.img");
                        let mut epos_file = File::create(&epos_path)
                            .map_err(|e| format!("Failed to create epos.img: {}", e))?;
                        epos_file
                            .write_all(&decompressed)
                            .map_err(|e| format!("Failed to write epos.img: {}", e))?;
                        println!("  Saved as: epos.img");

                        analyze_kernel(&decompressed);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to decompress {}: {}", name, e);
                }
            }
        }

        // 3. If name is melis-config, try to decompile to FEX
        if name == "melis-config" {
            println!("Decompiling config item: {}", name);
            match decompile_sys_config(payload) {
                Ok(fex_str) => {
                    let fex_path = dest_dir.join("sys_config.fex");
                    let mut fex_file = File::create(&fex_path)
                        .map_err(|e| format!("Failed to create fex file: {}", e))?;
                    fex_file
                        .write_all(fex_str.as_bytes())
                        .map_err(|e| format!("Failed to write fex file: {}", e))?;

                    // Write pin_mappings.md next to sys_config.fex
                    let pin_mappings = generate_pin_mappings_md(&fex_str);
                    let pin_path = dest_dir.join("pin_mappings.md");
                    let mut pin_file = File::create(&pin_path)
                        .map_err(|e| format!("Failed to create pin mappings file: {}", e))?;
                    pin_file
                        .write_all(pin_mappings.as_bytes())
                        .map_err(|e| format!("Failed to write pin mappings: {}", e))?;
                }
                Err(e) => {
                    eprintln!("Failed to decompile {}: {}", name, e);
                }
            }
        }
    }

    Ok(())
}
