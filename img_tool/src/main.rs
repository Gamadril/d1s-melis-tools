use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Processing mode
    #[arg(value_enum)]
    mode: Mode,
    /// The path to the input .img file to extract, or directory to pack
    input: PathBuf,
    /// The path to the output directory to extract to, or .img file to pack
    output: PathBuf,
    #[arg(short, long)]
    verbose: bool,
    /// Show what would be done without executing
    #[arg(short, long)]
    dry_run: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Mode {
    /// Extract an Allwinner IMAGEWTY (.img) file into its items + image.cfg
    Extract,
    /// Pack a directory (containing image.cfg and its listed items) into a .img file
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
                println!("    1. Parse IMAGEWTY header and item table");
                println!("    2. Extract every item to the output directory");
                println!("    3. Write image.cfg describing the layout");
            }
            Mode::Pack => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input directory: {}", args.input.display());
                println!(
                    "  Output file: {} (would be created)",
                    args.output.display()
                );
                println!("  Operations planned:");
                println!("    1. Read image.cfg from the input directory");
                println!("    2. Build the IMAGEWTY header and item table");
                println!("    3. Write header, item table and item payloads to the output file");
            }
        }
        return Ok(());
    }

    match args.mode {
        Mode::Extract => {
            if args.verbose {
                println!("Extracting Allwinner image: {}", args.input.display());
                println!("  Output directory: {}", args.output.display());
            } else {
                println!(
                    "Extracting image {:?} to {:?}",
                    args.input, args.output
                );
            }
            image::unpack_image(&args.input, &args.output)?;
            if args.verbose {
                println!("✅ Extraction complete!");
            }
        }
        Mode::Pack => {
            if args.verbose {
                println!("Packing Allwinner image from: {}", args.input.display());
                println!("  Output file: {}", args.output.display());
            } else {
                println!("Packing image {:?} to {:?}", args.input, args.output);
            }
            image::pack_image(&args.input, &args.output)?;
            if args.verbose {
                println!("✅ Packing complete!");
            } else {
                println!("Image packed successfully!");
            }
        }
    }

    Ok(())
}
