//! Host tool: patch eGON.BT0 checksum after objcopy.

use std::env;
use std::process::exit;

#[path = "../egon.rs"]
mod egon;

use egon::{patch_file, CHECKSUM_OFFSET};

fn main() {
    let path = match env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("Usage: egon_pad <binary>");
            exit(1);
        }
    };

    match patch_file(&path) {
        Ok((total, len)) => {
            println!("[OK] checksum = 0x{total:08x}  (offset 0x{CHECKSUM_OFFSET:02x})");
            println!("[OK] length   = {len} bytes = {} KiB", len / 1024);
            println!();
            println!("Flash with:");
            println!("    dd if={path} of=/dev/sdX bs=1024 seek=8");
        }
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    }
}
