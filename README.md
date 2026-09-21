# Allwinner F133/D1s Melis Firmware Toolkit

Custom tools to parse, extract, modify, and repack firmware for Allwinner F133/D1s (RISC-V) devices running Melis RTOS.

**Primary designed for:** Aftermarket automotive infotainment devices with CarPlay and Android Auto support, and similar consumer electronics based on the F133/D1s SoC running Melis RTOS.

**Note:** This toolkit should works with any F133/D1s Melis RTOS device - if not, create an issue and provide the dump/image. Device-specific reverse engineering notes and examples are documented separately.

Get the latest Release Build for your platform to start.

**Platform Notes:** Developed and tested primarily on Linux (Kubuntu). Windows and macOS support is included via helper scripts but less extensively tested.

> [!NOTE]
> If you land here because you just want to change the start image of your device - check `Quick Start: Change Startup Logo` in [docs/INFO.md](docs/INFO.md).

## Tools

Day-to-day binaries: **`dump_tool`** (firmware), **`data_renderer`** (`.data` UI), and **`compile_fex`** (recompile an edited `sys_config.fex`).
`melis-boot`, `minfs`, `gpt`, `udisk`, `image`, `dump`, and `melis-data` are internal libraries — not standalone tools themselves, though `melis-boot` is where `compile_fex` lives as a binary target.

You can use the compiled executables directly...
```bash
# Unpack full SPI NOR dump → boot0, GPT partitions, MinFS, UDISK, bootA (epos.img + sys_config.fex)
./dump_tool extract dump.bin out_dir

# Repack after editing unpacked tree (ROOTFS, UDISK, …). Editing bootA's
# sys_config.fex does NOT get picked up automatically - recompile it into
# melis-config.bin first (see docs/INFO.md, "Extra: Manually Recompiling
# sys_config.fex") if you changed it.
./dump_tool pack out_dir dump.repacked.bin
```

... or included helper scripts
```bash
./extract_dump.sh nor_dump.bin          # Linux / macOS
./pack_dump.sh [nor_repacked.bin]
```

```powershell
.\extract_dump.ps1 nor_dump.bin           # Windows
.\pack_dump.ps1 [nor_repacked.bin]
```

**Unpack layout** (under `out_dir/`):

```text
boot0.bin                   # eGON SPL, first stage (~49 KiB at SPI NOR offset 0)
gpt.bin.out/
  0_ppt.bin                 # GPT preamble (LBA 0–1): protective MBR + primary GPT header — not a data partition
  1_bootA.bin               # raw GPT partition image
  1_bootA.bin.out/          # TOC1 boot package
    epos.img                # OpenSBI + Melis kernel
    melis-config.bin
    sys_config.fex          # decompiled hardware config
    pin_mappings.md         # created sumary just for info
    melis-lzma.bin
  2_ROOTFS.bin              # raw GPT partition image
  2_ROOTFS.bin.out/         # MinFS (apps/, mod/, res/)
  3_UDISK.bin               # raw GPT partition image
  3_UDISK.bin.out/          # FAT16 user disk
```

`0_ppt.bin` is extracted for inspection only. `dump_tool pack` keeps the original `gpt.bin` header region and does not read `0_ppt.bin`. See [docs/INFO.md](docs/INFO.md) for layout details.

## Boot Chain

1. **boot0.bin** (~49 KiB, SPI NOR) — first stage
2. **GPT** — `1_bootA`, `2_ROOTFS` (MinFS), `3_UDISK` (FAT16)
3. **bootA** — TOC1 / `sunxi-package`: `melis-lzma` + `melis-config`
4. Kernel loads **epos.img**, mounts **D:** ROOTFS, **E:** UDISK, **F:** SDCARD, starts desktop

Sub-crate roles:

| Crate        | Role                                                |
|--------------|-----------------------------------------------------|
| `dump`       | Raw NOR dump header                                 |
| `gpt`        | Partition extract/splice                            |
| `melis-boot` | bootA TOC1 pack/unpack, LZMA extract, `sys_config` decompile/compile (also home of the `compile_fex` binary) |
| `minfs`      | ROOTFS pack/unpack                                  |
| `udisk`      | UDISK FAT16 pack/unpack                             |
| `image`      | eGON / CHK headers                                  |

## Release Bundle

Each archive's `bin/` directory contains `dump_tool`, `data_renderer`, `compile_fex`, and `xfel_spi_nor`.

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `melis-tools-linux-x86_64.tar.gz` |
| macOS (Apple Silicon) | `melis-tools-macos-aarch64.tar.gz` |
| Windows x86_64 | `melis-tools-windows-x86_64.zip` |

## Documentation

See the following documentation files for detailed technical information:

* [docs/INFO.md](docs/INFO.md) — Boot process, partition layout, reverse engineering, `.data` UI format, device-specific 
* [docs/PIN_MAPPING.md](docs/PIN_MAPPING.md) — F133 GPIO / mux reference

## `.data` UI tools

```bash
./data_renderer path/to/Main.data
./data_renderer --lang en path/to/Main.data
```

Opens a window for one `.data` and prints the widget/resource tree on stdout.
Quick viewer: 1:1 surface blit, not a pixel-accurate device compositor.
`--lang` picks a column from `apps/Language` when that directory sits next to `Data/`.

See [docs/INFO.md](docs/INFO.md) §7 for the `.data` format.
