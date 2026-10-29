use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use mcu_tool::{extract, Extraction, Options};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decode and validate an LTTMcu.bin companion MCU update (offline only)
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
    },
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
    let extraction = extract(&bytes, &Options { outer_only })?;
    if dry_run {
        print!("{}", extraction.manifest());
        println!("Dry run: no files written.");
    } else {
        write_extraction(&output, &extraction)?;
        println!(
            "Extracted {} artifacts to {} (model {:?}).",
            extraction.artifacts.len(),
            output.display(),
            extraction.wrapper.model
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
