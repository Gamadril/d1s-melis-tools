# Allwinner F133/D1s Melis Firmware Toolkit

Custom tools to parse, extract, modify, and repack firmware for Allwinner F133/D1s (RISC-V) devices running Melis RTOS.

**Primary designed for:** Aftermarket automotive infotainment devices with CarPlay and Android Auto support, and similar consumer electronics based on the F133/D1s SoC running Melis RTOS.

**Note:** This toolkit should works with any F133/D1s Melis RTOS device - if not, create an issue and provide the dump/image. Device-specific reverse engineering notes and examples are documented separately.

Get the latest Release Build for your platform to start.

**Platform Notes:** Developed and tested primarily on Linux (Kubuntu). Windows and macOS support is included via helper scripts but less extensively tested.

> [!NOTE]
> If you land here because you just want to change the start image of your device - check `Quick Start: Change Startup Logo` in [docs/INFO.md](docs/INFO.md).

## Tools

Only **`dump_tool`** is intended for day-to-day use.
The other crates (`melis-boot`, `minfs`, `gpt`, `udisk`, `image`, `dump`) are internal libraries wired into it.

You can use the compiled executables directly...
```bash
# Unpack full SPI NOR dump → boot0, GPT partitions, MinFS, UDISK, bootA (epos.img + sys_config.fex)
./dump_tool extract dump.bin out_dir

# Repack after editing unpacked tree (ROOTFS, UDISK, bootA sys_config.fex, …)
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
  1_bootA.bin.out/          # TOC1 boot package
    epos.img                # OpenSBI + Melis kernel
    melis-config.bin
    sys_config.fex          # decompiled hardware config
    pin_mappings.md         # created sumary just for info
    melis-lzma.bin
  2_ROOTFS.bin.out/         # MinFS (apps/, mod/, res/)
  3_UDISK.bin.out/          # FAT16 user disk
```

## Boot Chain

1. **boot0.bin** (~49 KiB, SPI NOR) — first stage
2. **GPT** — `1_bootA`, `2_ROOTFS` (MinFS), `3_UDISK` (FAT16)
3. **bootA** — TOC1 / `sunxi-package`: `melis-lzma` + `melis-config`
4. Kernel loads **epos.img**, mounts **D:** ROOTFS, **E:** UDISK, **F:** SDCARD, starts desktop

Sub-crate roles (internal only):

| Crate        | Role                                                |
|--------------|-----------------------------------------------------|
| `dump`       | Raw NOR dump header                                 |
| `gpt`        | Partition extract/splice                            |
| `melis-boot` | bootA TOC1, LZMA, `sys_config` decompile/patch/pack |
| `minfs`      | ROOTFS pack/unpack                                  |
| `udisk`      | UDISK FAT16 pack/unpack                             |
| `image`      | eGON / CHK headers                                  |

## Release Bundle

| Platform | Archive |
|----------|---------|
| Linux x86_64 | `melis-tools-linux-x86_64.tar.gz` |
| macOS (Apple Silicon) | `melis-tools-macos-aarch64.tar.gz` |
| Windows x86_64 | `melis-tools-windows-x86_64.zip` |

## Documentation

See the following documentation files for detailed technical information:

* [docs/INFO.md](docs/INFO.md) — Boot process, partition layout, reverse engineering, device-specific 
* [docs/PIN_MAPPING.md](docs/PIN_MAPPING.md) — F133 GPIO / mux reference
