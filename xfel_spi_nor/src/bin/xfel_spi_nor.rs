use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use xfel_spi_nor::{Error, SpinorFlash};

#[derive(Parser)]
#[command(name = "xfel_spi_nor", about = "D1/F133 SPI NOR over USB FEL")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// FEL chip id + SPI NOR detect
    Version,
    /// Detect SPI NOR (name + capacity)
    Spinor,
    /// Read SPI NOR to file or stdout (-)
    Read {
        address: u64,
        length: u64,
        file: PathBuf,
    },
    /// Write file to SPI NOR (erases covered sectors first, like xfel)
    Write { address: u64, file: PathBuf },
    /// Erase SPI NOR range
    Erase { address: u64, length: u64 },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Error> {
    let cli = Cli::parse();
    let mut dev = SpinorFlash::open()?;
    match cli.cmd {
        Cmd::Version => {
            println!("AWUSBFEX ID=0x{:08x}", dev.version_id());
            let info = dev.detect()?;
            println!("spinor {} ({} bytes)", info.name, info.capacity);
        }
        Cmd::Spinor => {
            let info = dev.detect()?;
            println!("{} {}", info.name, info.capacity);
        }
        Cmd::Read {
            address,
            length,
            file,
        } => {
            let mut buf = vec![0u8; length as usize];
            dev.read(address, &mut buf)?;
            if file.as_os_str() == "-" {
                std::io::stdout()
                    .write_all(&buf)
                    .map_err(|e| Error::Msg(e.to_string()))?;
            } else {
                std::fs::write(&file, &buf).map_err(|e| Error::Msg(e.to_string()))?;
            }
        }
        Cmd::Write { address, file } => {
            let mut f = File::open(&file).map_err(|e| Error::Msg(e.to_string()))?;
            let mut data = Vec::new();
            f.read_to_end(&mut data)
                .map_err(|e| Error::Msg(e.to_string()))?;
            dev.write(address, &data)?;
        }
        Cmd::Erase { address, length } => {
            dev.erase(address, length)?;
        }
    }
    Ok(())
}
