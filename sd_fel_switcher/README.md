# sd_fel_switcher

Rust `no_std` SPL flashed at **SD sector 16** (8 KiB). Boots from SD, tears down CCU like official `boot0`, jumps to BROM FEL @ `0x20` → USB `1f3a:efe8` (`xfel version`).

## Build

```bash
rustup target add riscv64imac-unknown-none-elf   # once
./build.sh              # → sd_fel_switcher.bin
```

Needs stable Rust and `riscv64-linux-gnu-objcopy` (linker is rustc’s default for the target).

## Flash

```bash
sudo dd if=sd_fel_switcher.bin of=/dev/sdX bs=1024 seek=8 conv=notrunc
```
where `sdX` is your card reader device - take care picking the right one.


## Layout

| Path | Role |
|------|------|
| `build.sh` | build / flash / clean / disasm |
| `src/main.rs` | eGON header (`global_asm!`) + FEL logic |
| `src/egon.rs`, `egon_pad` | eGON checksum (post-objcopy) |
| `link.ld` | linker script (SPL @ `0x00020000`; required by `.cargo/config.toml`) |

Bare-metal crate (workspace `exclude`); build from this directory.
