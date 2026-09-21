//! Compile a sys_config.fex text file into its binary form.
//!
//! Usage:
//!   compile_fex <sys_config.fex> <out.bin>
//!
//! Ships as a standalone binary in the release archives (bin/compile_fex),
//! same as dump_tool and data_renderer. During development you can also
//! run it via `cargo run -p melis-boot --bin compile_fex -- <fex> <out.bin>`.

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <sys_config.fex> <out.bin>", args[0]);
        process::exit(2);
    }
    let fex_text = match fs::read_to_string(&args[1]) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to read {}: {}", args[1], e);
            process::exit(1);
        }
    };
    match melis_boot::compile_sys_config(&fex_text) {
        Ok(bin) => {
            if let Err(e) = fs::write(&args[2], &bin) {
                eprintln!("Failed to write {}: {}", args[2], e);
                process::exit(1);
            }
            println!("Compiled {} bytes -> {}", bin.len(), args[2]);
        }
        Err(e) => {
            eprintln!("Compile error: {}", e);
            process::exit(1);
        }
    }
}
