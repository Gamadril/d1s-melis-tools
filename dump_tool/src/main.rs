use clap::{Parser, ValueEnum};
use dump::{extract_firmware, pack_firmware, write_dump_file};
use std::path::PathBuf;

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
                println!(
                    "  Output directory: {} (would be created)",
                    args.output.display()
                );
                println!("  Operations planned:");
                println!("    1. Extract boot0.bin and GPT");
                println!("    2. Extract 1_bootA (kernel)");
                println!("    3. Extract 2_ROOTFS (MinFS)");
                println!("    4. Extract 3_UDISK (FAT16)");
            }
            Mode::Pack => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input directory: {}", args.input.display());
                println!(
                    "  Output file: {} (would be created)",
                    args.output.display()
                );
                println!("  Operations planned:");
                println!("    1. Pack ROOTFS directory (2_ROOTFS.bin.out)");
                println!("    2. Pack UDISK directory (3_UDISK.bin.out)");
                println!("    3. Repack bootA with melis-config.bin as extracted");
                println!("    4. Splice all partitions into GPT image");
                println!("    5. Write final firmware image");
            }
        }
        return Ok(());
    }

    match args.mode {
        Mode::Extract => extract_firmware(&args.input, &args.output, args.verbose),
        Mode::Pack => {
            let packed = pack_firmware(&args.input, args.verbose)?;
            write_dump_file(&packed, &args.output, args.verbose)
        }
    }
}
