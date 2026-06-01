use clap::{Parser, ValueEnum};
use dump::unpack_dump;
use std::fs::File;
use std::path::PathBuf;

use gpt::extract_partitions;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Processing mode
    #[arg(value_enum)]
    mode: Mode,
    /// The path to the input file to extract or directory to pack
    input: PathBuf,
    /// The path to the output file to pack or directory to extract to
    output: PathBuf,
    #[arg(short, long)]
    verbose: bool,
    /// Show what would be done without executing
    #[arg(short, long)]
    dry_run: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Mode {
    /// Extract provided file
    Extract,
    /// Pack content of directory to file
    Pack,
}

fn main() -> Result<(), String> {
    let args = Args::parse();

    if args.dry_run {
        match args.mode {
            Mode::Extract => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input file: {}", args.input.display());
                println!("  Output directory: {} (would be created)", args.output.display());
                println!("  Operations planned:");
                println!("    1. Extract boot0.bin and GPT");
                println!("    2. Extract 1_bootA (kernel)");
                println!("    3. Extract 2_ROOTFS (MinFS)");
                println!("    4. Extract 3_UDISK (FAT16)");
            }
            Mode::Pack => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input directory: {}", args.input.display());
                println!("  Output file: {} (would be created)", args.output.display());
                println!("  Operations planned:");
                println!("    1. Pack ROOTFS directory (2_ROOTFS.bin.out)");
                println!("    2. Pack UDISK directory (3_UDISK.bin.out)");
                println!("    3. Repack bootA with sys_config.fex patches");
                println!("    4. Splice all partitions into GPT image");
                println!("    5. Write final firmware image");
            }
        }
        return Ok(());
    }

    if args.mode == Mode::Extract {
        if args.verbose {
            println!("Extracting firmware dump: {}", args.input.display());
        }
        unpack_dump(&args.input, &args.output)?;

        let mut gpt_in_path = PathBuf::from(&args.output);
        gpt_in_path.push("gpt.bin");
        let mut gpt_out_path = PathBuf::from(&args.output);
        gpt_out_path.push("gpt.bin.out");
        if args.verbose {
            println!("  Extracting GPT partitions from: {}", gpt_in_path.display());
        }
        extract_partitions(&gpt_in_path, &gpt_out_path)?;

        let mut rootfs_in_path = PathBuf::from(&gpt_out_path);
        rootfs_in_path.push("2_ROOTFS.bin");
        let mut rootfs_out_path = PathBuf::from(&gpt_out_path);
        rootfs_out_path.push("2_ROOTFS.bin.out");
        if args.verbose {
            println!("  Extracting ROOTFS (MinFS) from: {}", rootfs_in_path.display());
        }
        minfs::extract(&rootfs_in_path, &rootfs_out_path)?;

        let mut udisk_in_path = PathBuf::from(&gpt_out_path);
        udisk_in_path.push("3_UDISK.bin");
        let mut udisk_out_path = PathBuf::from(&gpt_out_path);
        udisk_out_path.push("3_UDISK.bin.out");
        if args.verbose {
            println!("  Extracting UDISK (FAT16) from: {}", udisk_in_path.display());
        }
        udisk::extract(&udisk_in_path, &udisk_out_path)?;

        let mut boot_in_path = PathBuf::from(&gpt_out_path);
        boot_in_path.push("1_bootA.bin");
        let mut boot_out_path = PathBuf::from(&gpt_out_path);
        boot_out_path.push("1_bootA.bin.out");
        if args.verbose {
            println!("  Extracting bootA (TOC1 package) from: {}", boot_in_path.display());
        } else {
            println!(
                "Extracting boot package {:?} to {:?}",
                boot_in_path, boot_out_path
            );
        }
        melis_boot::extract(&boot_in_path, &boot_out_path)?;
        if args.verbose {
            println!("✅ Extraction complete!");
        }
    } else if args.mode == Mode::Pack {
        if args.verbose {
            println!("Packing firmware image from: {}", args.input.display());
        }
        let input_dir = &args.input;
        let output_file = &args.output;

        // 1. Path to extracted rootfs directory
        let mut rootfs_dir = PathBuf::from(input_dir);
        rootfs_dir.push("gpt.bin.out");
        rootfs_dir.push("2_ROOTFS.bin.out");

        // 2. Pack the rootfs directory back to a new ROOTFS partition image
        let mut rootfs_repacked = PathBuf::from(input_dir);
        rootfs_repacked.push("gpt.bin.out");
        rootfs_repacked.push("2_ROOTFS.bin.repacked");
        if args.verbose {
            println!("  Packing MINFS directory: {}", rootfs_dir.display());
        } else {
            println!(
                "Packing MINFS directory {:?} to {:?}",
                rootfs_dir, rootfs_repacked
            );
        }
        minfs::pack(&rootfs_dir, &rootfs_repacked)?;

        // 3. Path to extracted udisk directory
        let mut udisk_dir = PathBuf::from(input_dir);
        udisk_dir.push("gpt.bin.out");
        udisk_dir.push("3_UDISK.bin.out");

        // 4. Pack the udisk directory back to a new UDISK partition image
        let mut udisk_repacked = PathBuf::from(input_dir);
        udisk_repacked.push("gpt.bin.out");
        udisk_repacked.push("3_UDISK.bin.repacked");
        let udisk_partition_size = 917504usize;
        if args.verbose {
            println!("  Packing UDISK directory: {}", udisk_dir.display());
        } else {
            println!(
                "Packing UDISK directory {:?} to {:?}",
                udisk_dir, udisk_repacked
            );
        }
        udisk::pack(&udisk_dir, &udisk_repacked, udisk_partition_size as u64)?;

        // 5. Repack bootA with patched sys_config
        let mut boot_out_dir = PathBuf::from(input_dir);
        boot_out_dir.push("gpt.bin.out");
        boot_out_dir.push("1_bootA.bin.out");

        let mut boot_template = PathBuf::from(input_dir);
        boot_template.push("gpt.bin.out");
        boot_template.push("1_bootA.bin");

        let mut boot_repacked = PathBuf::from(input_dir);
        boot_repacked.push("gpt.bin.out");
        boot_repacked.push("1_bootA.bin.repacked");

        let fex_path = boot_out_dir.join("sys_config.fex");
        let config_path = boot_out_dir.join("melis-config.bin");
        if fex_path.exists() {
            if args.verbose {
                println!("    Applying sys_config.fex UART patches to melis-config.bin");
            } else {
                println!("Applying sys_config.fex UART patches to melis-config.bin");
            }
            melis_boot::apply_sys_config_fex_patches(&fex_path, &config_path)?;
        }
        if args.verbose {
            println!("  Repacking bootA package");
        } else {
            println!(
                "Repacking bootA {:?} -> {:?}",
                boot_template, boot_repacked
            );
        }
        melis_boot::pack(&boot_template, &config_path, &boot_repacked)?;

        // 6. Read boot0.bin
        let mut boot0_path = PathBuf::from(input_dir);
        boot0_path.push("boot0.bin");
        if args.verbose {
            println!("  Reading boot0.bin");
        }
        let boot0_data =
            std::fs::read(&boot0_path).map_err(|e| format!("Error reading boot0.bin: {}", e))?;

        // 7. Read original gpt.bin to modify it
        let mut gpt_path = PathBuf::from(input_dir);
        gpt_path.push("gpt.bin");
        if args.verbose {
            println!("  Reading GPT image");
        }
        let mut gpt_data =
            std::fs::read(&gpt_path).map_err(|e| format!("Error reading gpt.bin: {}", e))?;

        // 8. Read repacked ROOTFS partition data
        let repacked_rootfs_data = std::fs::read(&rootfs_repacked)
            .map_err(|e| format!("Error reading repacked ROOTFS: {}", e))?;

        // 9. Read repacked UDISK partition data
        let repacked_udisk_data = std::fs::read(&udisk_repacked)
            .map_err(|e| format!("Error reading repacked UDISK: {}", e))?;

        // 10. Splice bootA
        let boota_offset = 0x4000usize;
        let boota_size = 1179648usize;
        let repacked_boota_data = std::fs::read(&boot_repacked)
            .map_err(|e| format!("Error reading repacked bootA: {}", e))?;
        if repacked_boota_data.len() > boota_size {
            return Err(format!(
                "Repacked bootA size ({} bytes) exceeds allocated partition size ({} bytes)",
                repacked_boota_data.len(),
                boota_size
            ));
        }
        if args.verbose {
            println!("  Splicing repacked bootA into GPT image");
        } else {
            println!(
                "Splicing repacked bootA into GPT image at offset {}",
                boota_offset
            );
        }
        gpt_data[boota_offset..(boota_offset + repacked_boota_data.len())]
            .copy_from_slice(&repacked_boota_data);
        if repacked_boota_data.len() < boota_size {
            let padding_start = boota_offset + repacked_boota_data.len();
            let padding_end = boota_offset + boota_size;
            for byte in &mut gpt_data[padding_start..padding_end] {
                *byte = 0;
            }
        }

        // 11. Splice ROOTFS
        let rootfs_offset = 1196032usize;
        let rootfs_size = 14614528usize;

        if repacked_rootfs_data.len() > rootfs_size {
            return Err(format!(
                "Repacked ROOTFS size ({} bytes) exceeds allocated partition size ({} bytes)",
                repacked_rootfs_data.len(),
                rootfs_size
            ));
        }

        if args.verbose {
            println!("  Splicing repacked ROOTFS into GPT image");
        } else {
            println!(
                "Splicing repacked ROOTFS into GPT image at offset {}",
                rootfs_offset
            );
        }
        gpt_data[rootfs_offset..(rootfs_offset + repacked_rootfs_data.len())]
            .copy_from_slice(&repacked_rootfs_data);

        if repacked_rootfs_data.len() < rootfs_size {
            let padding_start = rootfs_offset + repacked_rootfs_data.len();
            let padding_end = rootfs_offset + rootfs_size;
            for byte in &mut gpt_data[padding_start..padding_end] {
                *byte = 0;
            }
        }

        // 12. Splicing UDISK
        let udisk_offset = 15810560usize;
        let udisk_size = 917504usize;

        if repacked_udisk_data.len() > udisk_size {
            return Err(format!(
                "Repacked UDISK size ({} bytes) exceeds allocated partition size ({} bytes)",
                repacked_udisk_data.len(),
                udisk_size
            ));
        }

        if args.verbose {
            println!("  Splicing repacked UDISK into GPT image");
        } else {
            println!(
                "Splicing repacked UDISK into GPT image at offset {}",
                udisk_offset
            );
        }
        gpt_data[udisk_offset..(udisk_offset + repacked_udisk_data.len())]
            .copy_from_slice(&repacked_udisk_data);

        if repacked_udisk_data.len() < udisk_size {
            let padding_start = udisk_offset + repacked_udisk_data.len();
            let padding_end = udisk_offset + udisk_size;
            for byte in &mut gpt_data[padding_start..padding_end] {
                *byte = 0;
            }
        }

        // 13. Concatenate boot0_data and updated gpt_data to produce output_file
        if args.verbose {
            println!("  Writing final repacked firmware image");
        } else {
            println!("Writing final repacked image to {:?}", output_file);
        }
        use std::io::Write as _;
        let mut out_f =
            File::create(output_file).map_err(|e| format!("Error creating output file: {}", e))?;
        out_f
            .write_all(&boot0_data)
            .map_err(|e| format!("Error writing boot0: {}", e))?;
        out_f
            .write_all(&gpt_data)
            .map_err(|e| format!("Error writing gpt: {}", e))?;

        if args.verbose {
            println!("Packing complete!");
        } else {
            println!("Flash dump repacked successfully!");
        }
    }

    Ok(())
}
