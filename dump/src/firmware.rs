use std::fs::File;
use std::io::Write;
use std::path::Path;

use gpt::{extract_partitions, find_partition, read_partition_table};

use crate::unpack_dump;

/// Result of repacking a `dump_tool extract` tree: boot0 plus a GPT image
/// with bootA / ROOTFS / UDISK spliced back at this dump's own offsets.
pub struct PackedDump {
    pub boot0: Vec<u8>,
    pub gpt: Vec<u8>,
    pub boota: Vec<u8>,
    pub rootfs: Vec<u8>,
}

fn splice_partition(
    gpt_data: &mut [u8],
    offset: usize,
    size: usize,
    payload: &[u8],
    name: &str,
) -> Result<(), String> {
    if payload.len() > size {
        return Err(format!(
            "Repacked {} size ({} bytes) exceeds allocated partition size ({} bytes)",
            name,
            payload.len(),
            size
        ));
    }
    gpt_data[offset..(offset + payload.len())].copy_from_slice(payload);
    if payload.len() < size {
        gpt_data[(offset + payload.len())..(offset + size)].fill(0);
    }
    Ok(())
}

/// Unpack a raw NOR dump the same way `dump_tool extract` does: boot0, GPT
/// partitions, MinFS ROOTFS, FAT UDISK, TOC1 bootA.
pub fn extract_firmware(input: &Path, output: &Path, verbose: bool) -> Result<(), String> {
    if verbose {
        println!("Extracting firmware dump: {}", input.display());
    }
    unpack_dump(input, output)?;

    let gpt_in_path = output.join("gpt.bin");
    let gpt_out_path = output.join("gpt.bin.out");
    if verbose {
        println!(
            "  Extracting GPT partitions from: {}",
            gpt_in_path.display()
        );
    }
    extract_partitions(&gpt_in_path, &gpt_out_path)?;

    let rootfs_in_path = gpt_out_path.join("2_ROOTFS.bin");
    let rootfs_out_path = gpt_out_path.join("2_ROOTFS.bin.out");
    if verbose {
        println!(
            "  Extracting ROOTFS (MinFS) from: {}",
            rootfs_in_path.display()
        );
    }
    minfs::extract(&rootfs_in_path, &rootfs_out_path)?;

    let udisk_in_path = gpt_out_path.join("3_UDISK.bin");
    let udisk_out_path = gpt_out_path.join("3_UDISK.bin.out");
    if verbose {
        println!(
            "  Extracting UDISK (FAT16) from: {}",
            udisk_in_path.display()
        );
    }
    udisk::extract(&udisk_in_path, &udisk_out_path)?;

    let boot_in_path = gpt_out_path.join("1_bootA.bin");
    let boot_out_path = gpt_out_path.join("1_bootA.bin.out");
    if verbose {
        println!(
            "  Extracting bootA (TOC1 package) from: {}",
            boot_in_path.display()
        );
    } else {
        println!(
            "Extracting boot package {:?} to {:?}",
            boot_in_path, boot_out_path
        );
    }
    melis_boot::extract(&boot_in_path, &boot_out_path)?;
    if verbose {
        println!("✅ Extraction complete!");
    }
    Ok(())
}

/// Repack ROOTFS / UDISK / bootA from a `dump_tool extract` tree and splice
/// them into this dump's own GPT. Does not write a NOR image; callers write
/// `boot0 || gpt` (dump_tool) or wrap the blobs as IMAGEWTY items (img_tool).
pub fn pack_firmware(input_dir: &Path, verbose: bool) -> Result<PackedDump, String> {
    if verbose {
        println!("Packing firmware image from: {}", input_dir.display());
    }

    let gpt_path = input_dir.join("gpt.bin");
    let gpt_layout = read_partition_table(&gpt_path)?;
    let boota_part = find_partition(&gpt_layout, "bootA")?.clone();
    let rootfs_part = find_partition(&gpt_layout, "ROOTFS")?.clone();
    let udisk_part = find_partition(&gpt_layout, "UDISK")?.clone();
    if verbose {
        println!("  Layout read from {}:", gpt_path.display());
        for p in &gpt_layout {
            println!(
                "    {:<10} offset=0x{:08X} size=0x{:08X} ({} bytes)",
                p.name, p.offset, p.size, p.size
            );
        }
    }

    let gpt_out = input_dir.join("gpt.bin.out");

    let rootfs_dir = gpt_out.join("2_ROOTFS.bin.out");
    let rootfs_repacked = gpt_out.join("2_ROOTFS.bin.repacked");
    if verbose {
        println!("  Packing MINFS directory: {}", rootfs_dir.display());
    } else {
        println!(
            "Packing MINFS directory {:?} to {:?}",
            rootfs_dir, rootfs_repacked
        );
    }
    minfs::pack(&rootfs_dir, &rootfs_repacked)?;

    let udisk_dir = gpt_out.join("3_UDISK.bin.out");
    let udisk_repacked = gpt_out.join("3_UDISK.bin.repacked");
    if verbose {
        println!("  Packing UDISK directory: {}", udisk_dir.display());
    } else {
        println!(
            "Packing UDISK directory {:?} to {:?}",
            udisk_dir, udisk_repacked
        );
    }
    udisk::pack(&udisk_dir, &udisk_repacked, udisk_part.size)?;

    let boot_out_dir = gpt_out.join("1_bootA.bin.out");
    let boot_template = gpt_out.join("1_bootA.bin");
    let boot_repacked = gpt_out.join("1_bootA.bin.repacked");
    let config_path = boot_out_dir.join("melis-config.bin");
    if verbose {
        println!("  Repacking bootA package");
    } else {
        println!("Repacking bootA {:?} -> {:?}", boot_template, boot_repacked);
    }
    melis_boot::pack(&boot_template, &config_path, &boot_repacked)?;

    let boot0_path = input_dir.join("boot0.bin");
    if verbose {
        println!("  Reading boot0.bin");
    }
    let boot0 =
        std::fs::read(&boot0_path).map_err(|e| format!("Error reading boot0.bin: {}", e))?;

    if verbose {
        println!("  Reading GPT image");
    }
    let mut gpt_data =
        std::fs::read(&gpt_path).map_err(|e| format!("Error reading gpt.bin: {}", e))?;

    let rootfs = std::fs::read(&rootfs_repacked)
        .map_err(|e| format!("Error reading repacked ROOTFS: {}", e))?;
    let udisk = std::fs::read(&udisk_repacked)
        .map_err(|e| format!("Error reading repacked UDISK: {}", e))?;
    let boota = std::fs::read(&boot_repacked)
        .map_err(|e| format!("Error reading repacked bootA: {}", e))?;

    if verbose {
        println!("  Splicing repacked bootA into GPT image");
    } else {
        println!(
            "Splicing repacked bootA into GPT image at offset {}",
            boota_part.offset
        );
    }
    splice_partition(
        &mut gpt_data,
        boota_part.offset as usize,
        boota_part.size as usize,
        &boota,
        "bootA",
    )?;

    if verbose {
        println!("  Splicing repacked ROOTFS into GPT image");
    } else {
        println!(
            "Splicing repacked ROOTFS into GPT image at offset {}",
            rootfs_part.offset
        );
    }
    splice_partition(
        &mut gpt_data,
        rootfs_part.offset as usize,
        rootfs_part.size as usize,
        &rootfs,
        "ROOTFS",
    )?;

    if verbose {
        println!("  Splicing repacked UDISK into GPT image");
    } else {
        println!(
            "Splicing repacked UDISK into GPT image at offset {}",
            udisk_part.offset
        );
    }
    splice_partition(
        &mut gpt_data,
        udisk_part.offset as usize,
        udisk_part.size as usize,
        &udisk,
        "UDISK",
    )?;

    Ok(PackedDump {
        boot0,
        gpt: gpt_data,
        boota,
        rootfs,
    })
}

pub fn write_dump_file(
    packed: &PackedDump,
    output_file: &Path,
    verbose: bool,
) -> Result<(), String> {
    if verbose {
        println!("  Writing final repacked firmware image");
    } else {
        println!("Writing final repacked image to {:?}", output_file);
    }
    let mut out_f =
        File::create(output_file).map_err(|e| format!("Error creating output file: {}", e))?;
    out_f
        .write_all(&packed.boot0)
        .map_err(|e| format!("Error writing boot0: {}", e))?;
    out_f
        .write_all(&packed.gpt)
        .map_err(|e| format!("Error writing gpt: {}", e))?;
    if verbose {
        println!("Packing complete!");
    } else {
        println!("Flash dump repacked successfully!");
    }
    Ok(())
}
