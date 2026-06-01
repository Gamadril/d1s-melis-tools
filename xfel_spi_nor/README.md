# xfel_spi_nor

Rust port of [xfel](https://github.com/xboot/xfel) **SPI NOR read/write** for **Allwinner D1 / F133** only (USB FEL `1f3a:efe8`, RISC-V SPI payload from `chips/d1_f133.c`).

Workspace member of [melis_tools](../); release bundles ship `bin/xfel_spi_nor` plus `toolbox/xfel_spinor_{dump,recovery}.sh`.

## Build

From repo root:

```bash
cargo build --release -p xfel_spi_nor
```

Or from this directory: `cargo build --release`.

By default links **system libusb** (pkg-config / dev package). Static libusb for CI or machines without dev headers:

```bash
cargo build --release -p xfel_spi_nor --features vendored
```

Device must be in **FEL mode** (e.g. [sd_fel_switcher](../sd_fel_switcher/)). On Linux, USB access usually needs root or udev rules — the toolbox scripts call `sudo` when needed.

## Usage

```bash
xfel_spi_nor version          # AWUSBFEX id + detected NOR name/capacity
xfel_spi_nor spinor           # name + capacity (bytes)
xfel_spi_nor read 0 0x100000 dump.bin
xfel_spi_nor read 0 0x1000 -  # stdout
xfel_spi_nor write 0 image.bin   # erases covered sectors first (same as xfel spinor write)
xfel_spi_nor erase 0 0x10000
```

Read/write/erase show an xfel-style progress bar on **stderr when it is a TTY** (`NN% [====…] <rate> /s, ETA mm:ss`; final line shows total size and rate).

Full-dump helper (from release tarball or `toolbox/`):

```bash
./xfel_spinor_dump.sh [output.bin]   # Linux / macOS
```

```powershell
.\xfel_spinor_dump.ps1 [output.bin]    # Windows
```

## Library

```rust
let mut flash = xfel_spi_nor::SpinorFlash::open()?; // D1/F133 FEL only
let info = flash.detect()?;
flash.read(0, &mut buf)?;
flash.write(0, &data)?;
flash.erase(0, 0x10000)?;
```

## Scope

- **Included:** FEL USB protocol, D1 detect (`0x00185900` + SRAM signature), `chip_spi_init` / `chip_spi_run` payload, `spinor.c` detect/read/write/erase (SFDP + JEDEC fallback).
- **Excluded:** other SoCs, DDR, JTAG, SPI NAND, generic `xfel` memory commands.

Payload: `payloads/d1_spi_init.bin` (1480 B, extracted from upstream `d1_f133.c`).
