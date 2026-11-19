use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use mcu_tool::{extract, extract_flash, is_flash_image, Extraction, Options};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decode and validate an LTTMcu.bin update or a raw flash dump (offline only)
    Extract {
        input: PathBuf,
        /// New output directory; existing paths are refused
        output: PathBuf,
        /// Remove only the outer wrapper; do not interpret the inner package
        #[arg(long)]
        outer_only: bool,
        /// Decode and validate in memory without writing files
        #[arg(long)]
        dry_run: bool,
        /// Code key for a raw flash dump whose key is not the stock one;
        /// mcu_dump prints it after a dump
        #[arg(long, value_parser = parse_key)]
        code_key: Option<u32>,
    },
}

fn parse_key(text: &str) -> Result<u32, String> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    }
    .map_err(|e| format!("invalid code key {text:?}: {e}"))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn write_extraction(output: &Path, extraction: &Extraction) -> Result<(), String> {
    // All decoding/CRC checks have already succeeded. A fresh directory and
    // create_new files prevent overwriting prior work, including symlink targets.
    let manifest = extraction.manifest();
    fs::create_dir(output).map_err(|e| {
        format!(
            "cannot create new output directory {}: {e}",
            output.display()
        )
    })?;
    if extraction
        .artifacts
        .iter()
        .any(|artifact| artifact.path.starts_with("raw/"))
    {
        fs::create_dir(output.join("raw"))
            .map_err(|e| format!("cannot create raw output directory: {e}"))?;
    }
    for artifact in &extraction.artifacts {
        write_new(&output.join(&artifact.path), &artifact.bytes)?;
    }
    // Written last: an interrupted/failed write does not produce a success manifest.
    write_new(&output.join("manifest.txt"), manifest.as_bytes())
}

fn run() -> Result<(), String> {
    let Args { command } = Args::parse();
    let Command::Extract {
        input,
        output,
        outer_only,
        dry_run,
        code_key,
    } = command;
    match fs::symlink_metadata(&output) {
        Ok(_) => return Err(format!("output path already exists: {}", output.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => {
            return Err(format!(
                "cannot inspect output path {}: {e}",
                output.display()
            ))
        }
    }
    let bytes = fs::read(&input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    // A dump read off the chip is the same flash image an update file carries,
    // minus the wrapper, so the input decides which half of the decoder runs.
    let extraction = if is_flash_image(&bytes) {
        if outer_only {
            return Err("--outer-only applies to update files; a raw flash dump has no wrapper".into());
        }
        extract_flash(&bytes, code_key)?
    } else {
        if code_key.is_some() {
            return Err(
                "--code-key applies to a raw flash dump; an update file carries its own key".into(),
            );
        }
        extract(&bytes, &Options { outer_only })?
    };
    if dry_run {
        print!("{}", extraction.manifest());
        println!("Dry run: no files written.");
    } else {
        write_extraction(&output, &extraction)?;
        let kind = match &extraction.wrapper {
            Some(wrapper) => format!("model {:?}", wrapper.model),
            None => "raw flash dump".into(),
        };
        println!(
            "Extracted {} artifacts to {} ({kind}).",
            extraction.artifacts.len(),
            output.display(),
        );
        println!(
            "Validation results, hashes and source offsets: {}",
            output.join("manifest.txt").display()
        );
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mcu_tool: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
