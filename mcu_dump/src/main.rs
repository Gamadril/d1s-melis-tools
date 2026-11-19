//! Dump the internal flash of a Bluetrum AB5301B ("BLUECRWN") companion MCU
//! through its mask-ROM UART bootloader, following `docs/CRWN_DUMP_PROTOCOL.md`.
//!
//! The ROM cannot read the memory-mapped flash itself, so a 412-byte RAM stub is
//! uploaded to $12000 and installed as the command handler; it reads the on-die
//! flash over the chip's internal SPI0 bus. Read-only: no write, no erase.
//! The dump is scrambled; `mcu_tool extract` decodes it.

mod proto;

use std::fs::OpenOptions;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

use proto::{command_block, Link, Result, BLOCK};

/// The stub is linked at this address; the chip must report the same one.
const LOAD_ADDR: u32 = 0x1_2000;
const STUB: &[u8] = include_bytes!("crwn_stub.bin");
/// Largest flash read per command (`arg3` is a u16, and the stub streams it).
const MAX_CHUNK: usize = 0x8000;

#[derive(Parser)]
#[command(version, about = "Dump AB5301B (BLUECRWN) internal flash over UART")]
struct Args {
    /// Serial port of the UART adapter, e.g. /dev/ttyUSB0 or COM3
    #[arg(long)]
    port: String,
    /// New output file for the raw dump; existing paths are refused
    output: PathBuf,
    /// Baud rate the bootloader starts at
    #[arg(long, default_value_t = 115_200)]
    init_baud: u32,
    /// Baud rate to switch to for the transfer (same as --init-baud: no switch)
    #[arg(long, default_value_t = 921_600)]
    baud: u32,
    /// First flash address to read
    #[arg(long, default_value = "0", value_parser = parse_int)]
    offset: u32,
    /// Bytes to read (default: to the end of the flash)
    #[arg(long, value_parser = parse_int)]
    size: Option<u32>,
    /// Reboot the chip when the dump is done
    #[arg(long)]
    reboot: bool,
}

fn parse_int(text: &str) -> std::result::Result<u32, String> {
    let text = text.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|e| format!("invalid number {text:?}: {e}"))
}

/// Percentage on stderr, like the other hardware tool in this tree.
struct Progress {
    total: u64,
    done: u64,
    start: Instant,
    show: bool,
}

impl Progress {
    fn start(total: u64) -> Self {
        Self {
            total,
            done: 0,
            start: Instant::now(),
            show: std::io::stderr().is_terminal(),
        }
    }

    fn update(&mut self, bytes: u64) {
        self.done += bytes;
        if !self.show {
            return;
        }
        let rate = self.done as f64 / self.start.elapsed().as_secs_f64().max(1e-9) / 1024.0;
        let percent = 100.0 * self.done as f64 / self.total.max(1) as f64;
        let mut err = std::io::stderr();
        let _ = write!(
            err,
            "\rReading {percent:3.0}% ({}/{} KiB, {rate:.1} KiB/s)   ",
            self.done / 1024,
            self.total / 1024
        );
        let _ = err.flush();
    }

    fn stop(self) {
        if self.show {
            let _ = writeln!(std::io::stderr());
        }
    }
}

/// GET_INFO, AUTHORIZE, optional baud switch, stub upload (§5, §7).
fn open_session(args: &Args) -> Result<Link> {
    let mut link = Link::open(&args.port, args.init_baud)?;
    eprintln!("Synchronizing with the bootloader on {}...", args.port);
    link.sync(args.init_baud, args.baud)?;

    let info = link.exec(
        command_block(proto::GET_INFO, 0x5259_414e, 0, 0x67ca),
        None,
        24,
        None,
    )?;
    if info.len() < 24 {
        return Err(format!("short GET_INFO response ({} bytes)", info.len()));
    }
    let chip_id = &info[..12];
    if !chip_id.starts_with(b"BLUECRWN") {
        return Err(format!(
            "unsupported chip {:?}: this tool only handles BLUECRWN (AB53xx) parts",
            String::from_utf8_lossy(chip_id)
        ));
    }
    let load_addr = u32::from_be_bytes(info[12..16].try_into().unwrap());
    let commkey = u32::from_be_bytes(info[16..20].try_into().unwrap());
    println!("Chip ID:      {}", String::from_utf8_lossy(&chip_id[..8]));
    println!("Load address: {load_addr:#010x}");
    if load_addr != LOAD_ADDR {
        return Err(format!(
            "chip wants the handler at {load_addr:#x}, but the stub is linked at {LOAD_ADDR:#x}"
        ));
    }

    let authorized = link.exec(
        command_block(proto::AUTHORIZE, proto::calckey(commkey), 0, 0),
        None,
        4,
        None,
    )?;
    if authorized.iter().all(|&b| b == 0) {
        return Err("authorization refused (the chip returned a null key)".into());
    }

    if args.baud != args.init_baud {
        eprintln!("Switching to {} baud...", args.baud);
        // Switch to a faster clock reference first, then the divider (§5.5).
        link.exec(command_block(proto::IFACE_PARAM, 0, 0xf0, 0), None, 2, None)?;
        link.exec(
            command_block(proto::IFACE_PARAM, args.baud, 0x02, 0),
            None,
            2,
            Some(args.baud),
        )?;
    }

    let mut stub = STUB.to_vec();
    stub.resize(stub.len().div_ceil(BLOCK) * BLOCK, 0);
    let blocks = (stub.len() / BLOCK) as u16;
    link.exec(
        command_block(proto::MEM_WRITE, LOAD_ADDR, 0, blocks),
        Some(&stub),
        0,
        None,
    )?;
    let written = link.exec(
        command_block(proto::MEM_READ, LOAD_ADDR, 0, blocks),
        None,
        stub.len(),
        None,
    )?;
    if written != stub {
        return Err("the uploaded stub did not read back correctly".into());
    }
    link.exec(
        command_block(proto::SET_CMD_HANDLER, LOAD_ADDR, 0, 0),
        None,
        0,
        None,
    )?;
    Ok(link)
}

fn run() -> Result<()> {
    let args = Args::parse();
    // Refuse to clobber an earlier dump; the only copy of a stock image matters.
    // Checked up front, but created below, so a failed session leaves no stray
    // file to delete before the next attempt.
    match std::fs::symlink_metadata(&args.output) {
        Ok(_) => {
            return Err(format!(
                "output path already exists: {}",
                args.output.display()
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => {
            return Err(format!(
                "cannot inspect output path {}: {e}",
                args.output.display()
            ))
        }
    }

    let mut link = open_session(&args)?;

    // INIT sets up the SPI pins and wakes the flash; it must precede any read.
    let init = link.exec(command_block(proto::STUB_INIT, 0, 0, 0), None, 48, None)?;
    if init.len() < 24 {
        return Err(format!("short INIT response ({} bytes)", init.len()));
    }
    let codekey = u32::from_le_bytes(init[0..4].try_into().unwrap());
    let jedec = u32::from_le_bytes(init[4..8].try_into().unwrap());
    let uid: String = init[8..24].iter().map(|b| format!("{b:02x}")).collect();
    println!("Flash JEDEC:  {jedec:#08x}");
    println!("Flash UID:    {uid}");

    let density = jedec & 0xff;
    let flash_size = match density {
        0x10..=0x18 => 1u32 << density,
        _ => 0,
    };
    if flash_size == 0 {
        println!("Flash size:   unknown (density byte {density:#04x})");
    } else {
        println!("Flash size:   {flash_size} bytes");
    }

    let size = match args.size {
        Some(size) => size,
        None => flash_size
            .checked_sub(args.offset)
            .filter(|size| *size > 0)
            .ok_or("unknown flash size: pass --size")?,
    };
    if size == 0 {
        return Err("nothing to read: --size is 0".into());
    }
    if flash_size > 0 && args.offset.saturating_add(size) > flash_size {
        return Err(format!(
            "{size} bytes at {:#x} run past the end of the {flash_size}-byte flash",
            args.offset
        ));
    }

    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)
        .map_err(|e| format!("cannot create {}: {e}", args.output.display()))?;

    // One chunk per percent of the dump, capped, so progress moves steadily.
    let chunk = ((size as usize / 100).div_ceil(BLOCK) * BLOCK).clamp(BLOCK, MAX_CHUNK);
    eprintln!("Reading {size} bytes from {:#x}...", args.offset);
    let mut progress = Progress::start(u64::from(size));
    let mut done = 0u32;
    let result = loop {
        if done >= size {
            break Ok(());
        }
        let want = chunk.min((size - done) as usize);
        let block = match link.exec(
            command_block(proto::STUB_READ, args.offset + done, 0, want as u16),
            None,
            want,
            None,
        ) {
            Ok(block) => block,
            Err(e) => break Err(e),
        };
        if block.len() != want {
            break Err(format!(
                "short read at {:#x}: wanted {want} bytes, got {}",
                args.offset + done,
                block.len()
            ));
        }
        if let Err(e) = output.write_all(&block) {
            break Err(format!("cannot write {}: {e}", args.output.display()));
        }
        done += want as u32;
        progress.update(want as u64);
    };
    progress.stop();
    output
        .flush()
        .map_err(|e| format!("cannot write {}: {e}", args.output.display()))?;
    result?;

    if args.reboot {
        link.exec(command_block(proto::REBOOT, 0, 0, 0), None, 0, None)?;
    }

    println!("Wrote {done} bytes to {}", args.output.display());
    println!("Code key:     {codekey:#010x}");
    // mcu_tool recovers the stock key on its own; --code-key covers the rest.
    println!(
        "The dump is scrambled. Decode it with:\n  mcu_tool extract {} {}-extracted",
        args.output.display(),
        args.output.display()
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mcu_dump: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
