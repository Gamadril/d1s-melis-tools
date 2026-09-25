use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

mod nested;

const DEFAULT_IMG_NAME: &str = "LTTF133.img";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Processing mode
    #[arg(value_enum)]
    mode: Mode,
    /// Input .img (extract), IMAGEWTY extract dir (pack), or dump_tool extract dir (from-dump)
    input: PathBuf,
    /// Output directory (extract) or .img file (pack / from-dump).
    /// Pack / from-dump default: LTTF133.img
    output: Option<PathBuf>,
    #[arg(short, long)]
    verbose: bool,
    /// Show what would be done without executing
    #[arg(short, long)]
    dry_run: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Mode {
    /// Extract an Allwinner IMAGEWTY (.img) file into its items + nested bootA/MinFS
    Extract,
    /// Pack an img_tool extract directory (image.cfg + items) back into a .img
    Pack,
    /// Build a .img from a dump_tool extract tree
    FromDump,
}

fn resolve_output(mode: Mode, output: Option<PathBuf>) -> Result<PathBuf, String> {
    match output {
        Some(path) => Ok(path),
        None => match mode {
            Mode::Extract => Err(
                "extract requires an output directory (pack / from-dump default to LTTF133.img)"
                    .into(),
            ),
            Mode::Pack | Mode::FromDump => Ok(PathBuf::from(DEFAULT_IMG_NAME)),
        },
    }
}

fn print_sd_hint(output: &Path) {
    println!(
        "Copy {} onto the SD/USB card as /Update/{} (HZ-B500: Config.ini appUpdateFile can rename it).",
        output.display(),
        DEFAULT_IMG_NAME
    );
}

fn main() -> Result<(), String> {
    let args = Args::parse();
    let output = resolve_output(args.mode, args.output)?;

    if args.dry_run {
        match args.mode {
            Mode::Extract => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input file: {}", args.input.display());
                println!(
                    "  Output directory: {} (would be created)",
                    output.display()
                );
                println!("  Operations planned:");
                println!("    1. Parse IMAGEWTY header and item table");
                println!("    2. Extract every item to the output directory");
                println!("    3. Write image.cfg describing the layout");
                println!("    4. Extract TOC1 (bootA) and MinFS (ROOTFS) items to <file>.out/");
            }
            Mode::Pack => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input directory: {}", args.input.display());
                println!(
                    "  Output file: {} (would be created)",
                    output.display()
                );
                println!("  Operations planned:");
                println!("    1. Repack nested bootA/MinFS <file>.out/ trees into item files");
                println!("    2. Refresh V* verify checksums if present");
                println!("    3. Read image.cfg and write IMAGEWTY output");
            }
            Mode::FromDump => {
                println!("🔍 DRY RUN MODE: No files will be modified");
                println!("  Input directory: {}", args.input.display());
                println!(
                    "  Output file: {} (would be created)",
                    output.display()
                );
                println!("  Operations planned:");
                println!("    1. Pack ROOTFS / UDISK / bootA (dump_tool behaviour)");
                println!("    2. Build sunxi_mbr.fex + dlinfo.fex from this dump's GPT");
                println!("    3. Wrap boot0 + GPT + MBR + bootA + ROOTFS as IMAGEWTY");
            }
        }
        return Ok(());
    }

    match args.mode {
        Mode::Extract => {
            if args.verbose {
                println!("Extracting Allwinner image: {}", args.input.display());
                println!("  Output directory: {}", output.display());
            } else {
                println!("Extracting image {:?} to {:?}", args.input, output);
            }
            image::unpack_image(&args.input, &output)?;
            nested::extract_nested_items(&output, args.verbose)?;
            if args.verbose {
                println!("✅ Extraction complete!");
            }
        }
        Mode::Pack => {
            if args.verbose {
                println!("Packing Allwinner image from: {}", args.input.display());
                println!("  Output file: {}", output.display());
            } else {
                println!("Packing image {:?} to {:?}", args.input, output);
            }
            if !nested::is_img_extract_dir(&args.input) {
                return Err(format!(
                    "Input {:?} has no image.cfg (use from-dump for a dump_tool extract tree)",
                    args.input
                ));
            }
            nested::pack_nested_items(&args.input, args.verbose)?;
            image::pack_image(&args.input, &output)?;
            if args.verbose {
                println!("✅ Packing complete!");
            } else {
                println!("Image packed successfully!");
            }
            print_sd_hint(&output);
        }
        Mode::FromDump => {
            if args.verbose {
                println!(
                    "Building Allwinner image from dump extract: {}",
                    args.input.display()
                );
                println!("  Output file: {}", output.display());
            } else {
                println!(
                    "Building image from dump {:?} to {:?}",
                    args.input, output
                );
            }
            if !nested::is_dump_extract_dir(&args.input) {
                return Err(format!(
                    "Input {:?} is not a dump_tool extract (need boot0.bin + gpt.bin)",
                    args.input
                ));
            }
            nested::pack_dump_dir_as_image(&args.input, &output, args.verbose)?;
            if args.verbose {
                println!("✅ Packing complete!");
            } else {
                println!("Image packed successfully!");
            }
            print_sd_hint(&output);
        }
    }

    Ok(())
}
