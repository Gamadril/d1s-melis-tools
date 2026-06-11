# Allwinner F133/D1s Melis Firmware Information

Some info collected during reverse engineering of the device firmware.
Description is based on the **HZ-B500-MB** device with 16MB SPI NOR flash, Melis RTOS and proprietary UI.

---

## 1. Boot Process Overview

Device boot sequence:

```mermaid
graph TD
    A[Power On / Reset] --> B[BootROM]
    B --> C[Load & Verify boot0.bin into SRAM]
    C --> D[boot0.bin SPL]
    D --> E[Init Clocks, DDR, GPIO]
    E --> F[Parse GPT & Load TOC1 bootA / bootB]
    F --> G[Extract & Decompress melis-lzma]
    G --> H[Jump to 0x40000000]
    H --> I[OpenSBI Init]
    I --> J[Jump to Melis Kernel]
    J --> K[Mount ROOTFS MinFS & UDISK FAT16]
    K --> L[Start System Tasks]
```

### Stage 1: BootROM (Hardcoded)
Primary hardware bootstrap.
Divided in 2 parts: FEL module (USB endpoint for low level operations) and Medium Boot module (loads BOOT0 from flash/SD card and runs it).
Executes from internal ROM (at address 0x0).

### Stage 2: Boot0 / SPL (Secondary Program Loader)
Responsible for DRAM and basic board initialization.
Reads the GPT partition table to locate the boot partition (`1_bootA` / `1_bootB`), which is packaged in the **TOC1** (`sunxi-package`) container format.
*   For unpacking:
    *   Parses the TOC1 package directory.
    *   Extracts the binary hardware configuration (`melis-config.bin` / `sys_config.bin`).
    *   Extracts and decompresses the main OS kernel (`melis-lzma.bin`) using LZMA into DRAM at `0x40000000`.
Jumps to `0x40000000` for execution.

### Stage 3: OpenSBI & Melis RTOS Kernel
Supervisor Environment Setup & Operating System execution.
*   **OpenSBI**:
    *   Linked at the start of the decompressed kernel image.
    *   Installs M-mode traps, emulation layers, and privilege switch handlers.
    *   Hands off execution to the Melis OS kernel in S-mode.
*   **Melis RTOS**:
    *   Initializes kernel services, drivers, and peripheral interfaces.
    *   Mounts the system partitions: **`2_ROOTFS`** (**MinFS**) and **`3_UDISK`** (**FAT16**).
    *   Spawns UI/system services.

---

## 2. Partition Layout

Based on the flash dump `dump_hz_b500_1_7.bin`:

| Image File | Offset | Size (Bytes) | Format | Description / Role |
| :--- | :--- | :--- | :--- | :--- |
| **`boot0.bin`** | `0x00000000` | `49,152` | eGON.BT0 | Primary bootloader (SPL) |
| **`gpt.bin`** | `0x0000c000` | Variable | GPT disk image | Preamble (`0_ppt.bin`) + partitions `1_bootA`, `2_ROOTFS`, `3_UDISK` |
| **`0_ppt.bin`** | Start of `gpt.bin` | `1,024` | GPT preamble | First two 512-byte LBAs: protective MBR (LBA 0) + primary GPT header beginning with `EFI PART` (LBA 1). Allwinner tooling label **PPT** (partition table); not userdata — leave unchanged when patching |
| **`1_bootA.bin`** | Inside GPT | Variable | TOC1 / sunxi-package | Boot package containing kernel payload & config |
| **`2_ROOTFS.bin`** | Inside GPT | `14,614,528` | MinFS | Allwinner proprietary RTOS filesystem (modules, apps, configurations) |
| **`3_UDISK.bin`** | Inside GPT | `917,504` | FAT16 | User disk partition containing resources and config scripts |

---

## 3. Extracted Sub-Components Detail

### `0_ppt.bin` (GPT Preamble)

Extracted from the first 1024 bytes of `gpt.bin` before numbered GPT partitions are read.

| Byte range | GPT role | Typical content |
| :--- | :--- | :--- |
| `0x000`–`0x1FF` | Protective MBR (LBA 0) | Often all zeros on SPI NOR Melis images |
| `0x200`–`0x3FF` | Primary GPT header (LBA 1) | Signature `EFI PART`, partition entry pointers, disk GUID |

*   **Not** a mountable firmware partition — unlike `1_bootA`, `2_ROOTFS`, or `3_UDISK`.
*   The `0_` prefix marks it as outside the GPT partition entry list (`1_` … `3_` come from partition names in the table).
*   **PPT** = legacy Allwinner name for the partition-table area (sunxi MBR era); on F133/D1s the layout is GPT, but the extractor keeps the old filename.
*   **Repacking:** `dump_tool pack` splices modified partition images into the original `gpt.bin` and preserves this header block automatically. You do not need to edit or restore `0_ppt.bin` for normal ROOTFS / UDISK / `sys_config.fex` workflows.

### `1_bootA.bin.out/` (TOC1 Container Output)
*   **`melis-config.bin`**: The binary-compiled `sys_config.bin` containing hardware configurations. Can be decompiled to human-readable FEX format.
*   **`melis-config.bin.out/sys_config.fex`**: The decompiled hardware layout (GPIO pins, interface modes, system parameters).
*   **`melis-lzma.bin`**: Raw compressed LZMA kernel package.
*   **`melis-lzma.decompressed`**: The decompressed kernel image. Begins with the OpenSBI wrapper (containing the `"opensbi"` metadata signature) linked directly to the Melis RTOS kernel.

### `2_ROOTFS.bin.out/` (MinFS Filesystem Output)
*   Holds the core components of the system.
*   Contains dynamic modules (`.mod` or `.drv`), GUI images, system config files, and core applications.
*   Extracted and repacked using the workspace `minfs` tool.

### `3_UDISK.bin.out/` (FAT16 Partition Output)
*   Used for user-space resource storage, configuration scripts, logs, or secondary assets.
*   Extracted and repacked using the workspace `udisk` tool.

---

## 4. Reversing, Patching, and Flashing

### Decompiling & Modifying Configs
1.  Extract the firmware using `dump_tool extract`.
2.  Open `sys_config.fex` inside `1_bootA.bin.out/`.
3.  Modify hardware parameters (e.g. enabling debugging consoles, swapping functional GPIO pins).
4.  *(Note: Pack tools will compile `.fex` back to binary automatically).*

### Patching the OS Kernel & Modules
In theory patching the kernel should be possible, but since it's a big binary blob not very practical.
I think it might be easier to use the Melis kernel compiled from the source code since it's acting as HAL - main functions like AA and CarPlay are in UI/ROOTFS modules.
Ghidra could help analyzing the existing decompressed kernel.
D1s is based on T-Head Xuantie C906 core. There is some effort to support T-Head extensions in Ghidra: ([Issue #5778](https://github.com/NationalSecurityAgency/ghidra/pull/5778))

### Repacking the System Filesystem
1.  Modify contents within `2_ROOTFS.bin.out/` or `3_UDISK.bin.out/`.
2.  Use the repacking functionality:
    ```bash
    dump_tool pack <input_dir> <output_dump.bin>
    ```
    This automatically:
    *   Repacks the `2_ROOTFS.bin.out/` back into MinFS.
    *   Repacks the `3_UDISK.bin.out/` back into FAT16.
    *   Splices the repacked partition binaries back into the GPT image (`gpt.bin`) at their original offsets.
    *   Prepends `boot0.bin` to build the full flash image ready for writing.

---

## 4. Some device hacks

### Quick Start: Change Startup Logo

If you only want to change the boot-up logo **without modifying the firmware**:

1. Format an SD card as FAT32
2. Create a `Logo` directory on the card
3. Copy your JPEG image to `Logo/`
4. Insert the SD card into the device
5. Enter factory settings using code `112233`
6. Click "Internal card" text to switch to "External card"
7. Select your image to set it as the boot logo
8. Exit the menu — the image will be copied to UDISK as `stalogo.jpg`
9. Remove the SD card and reboot

### Factory Settings Passwords (HZ-B500-MB)

In the **Settings** menu, **Factory settings** entry asks for a 6-digit code. Each code opens a different hidden configuration screen.

**Source**: Passwords are defined in `2_ROOTFS.bin.out/apps/init.axf` (main shell / login UI). Password checks are hardcoded strings plus two values from `apps/Config.ini` (also on UDISK).

| Code | Menu (English) | Config / Notes |
|------|----------------|----------------|
| `112233` | Logo settings | `logoPassword` in `[CONFIG]` (default in firmware) → `SetupLogo.data` |
| `113266` | Factory settings | `factoryPaswword` in `[CONFIG]` (typo in stock INI) → `SetupFactory.data` |
| `112345` | Debug mode | Hardcoded → `SetupDebug.data` |
| `001106` | Factory settings (extended) | Hardcoded; menu id 25 (separate code path from `113266`) |
| `230762` | Interface selection | Hardcoded; UI style / layout picker (`uiType` / `uiID` area) |
| `123579` | Self-check | Hardcoded; minimal UI (can look "empty") |

Other digit strings exist in `init.axf` (WiFi defaults, version blobs, etc.) but are **not** wired into this login dispatcher.

---

## 5. Flash Dump: Creation and Recovery (HZ-B500-MB)

### Storage Media & Boot Priority

F133 supports multiple boot media: SPI NAND, SPI NOR, SD card, and eMMC.
HZ-B500-MB uses a 16MB **SPI NOR flash** for system boot.

The device supports firmware dump and recovery without opening the case using **FEL (Firmware Exchange Launch)** mode.

### FEL Mode Entry

F133 can enter FEL (a low-level bootloader mode) via USB if no valid boot media is found. The simplest approach for HZ-B500-MB is to use an SD card with a small boot image:

1.  Boot the device from the SD card → device enters FEL automatically
2.  Connect device to host via USB-A to USB-A cable
3.  Use `xfel_spi_nor` tool for read/write/erase operations

### Dump Flash via FEL

**Prerequisites:**
*   SD card (any size)
*   USB-A to USB-A cable
*   `sd_fel_switcher.bin` (included in toolkit)

**Steps:**

1. Write the FEL boot image to SD card:
```bash
sudo dd if=sd_fel_switcher/sd_fel_switcher.bin of=/dev/sdX bs=1024 seek=8 conv=notrunc
```
(Replace `sdX` with your card's device name — be careful!)

2. Insert SD card into device and connect USB cable to host

3. Device will boot and enter FEL automatically

4. Verify device detection:
```bash
lsusb | grep "Allwinner\|D1s"
```

5. Create a flash dump:
```bash
./xfel_spinor_dump.sh
```
This creates a `nor_dump_DATE.bin` file

### Recovery: Write Dump Back to Flash

To restore a previously dumped firmware:

```bash
./xfel_spinor_recovery.sh path/to/nor_dump.bin
```

> [!IMPORTANT]
> **Backup your original unmodified dump!** Using FEL and your original dump, you can unbrick the device if modifications go wrong. Keep a safe backup.

---

## 6. Device Debugging

### Serial Console (UART0 Debug)

The HZ-B500-MB has a test point labeled **"TX"** on the board. This is the SoC's **UART0 TX
output** — the pin is **PE2** (`UART0-TX`).

> [!IMPORTANT]
> **Pin assignment:**
> - **PE2** = `UART0-TX` (SoC transmit → your adapter RX) — this is the "TX" test point
> - **PE3** = `UART0-RX` (SoC receive → your adapter TX) — **no test point exposed**

**Setup:**
- Attach a **3.3V TTL** serial adapter (e.g. USB-to-UART)
- Connect adapter RX → board "TX" test point (PE2)
- Connect adapter TX → PE3 (requires soldering directly to the IC pin — see note below)
- Baud rate: **500,000** (500K) 8N1
- Provides: boot logs, kernel debug (`[DBG]`/`[ERR]` messages), FinSH shell (`msh />`)

**Note on RX (PE3)**: There is no "RX" test point. For bidirectional communication (to send
commands to the FinSH shell), you must solder directly to IC pin PE3 **and** enable UART0 RX
in `sys_config.fex` under `[uart_para]` + `[uart0]`.

### Serial Interfaces Table

| UART | Pins | FEX Config | Baud | Purpose |
|------|------|--------|------|---------|
| **UART0** (debug) | PE02 TX, PE03 RX | `[uart_para]` + `[uart0]` | **500000** 8N1 | FinSH shell (`msh />`), printk logs, debug messages (`[DBG]`/`[ERR]`) |
| **UART4** (MCU) | PE04 TX, PE05 RX | `[uart4]`, `Config.ini` → `mcuSerialName=/dev/uart4` | 115200 8N1 | Front-panel MCU communication — binary protocol (`02 80 …`), `CMcuCtrl` subsystem |
| **UART1** | PG12/PG13 | `[uart1]` in fex | — | **Unused** on this firmware; pins conflict with disabled `daudio1` I2S; wired to connector J6 |

### Apple CarPlay Authentication (MFi Coprocessor)

Wireless CarPlay support requires a physical **Apple MFi Authentication Coprocessor** (such as `CP2.0` or `CP3.0`). This coprocessor performs cryptographic handshakes for CarPlay sessions.

**Hardware Integration:**
*   **Component**: U31 label on HZ-B500-MB board
*   **Interface**: **TWI2** (I2C) bus
*   **Pins**: PE12 (SCL), PE13 (SDA)
*   **FEX Config**: Defined under `[twi2]` section in `sys_config.fex`

**Software Configuration:**
*   **Config file**: `Config.ini` (UDISK partition)
*   **Key**: `carplayI2cName=/dev/twi2` in the `[LINK]` section

**Discovery Notes:**
*   The exact I2C slave address of the MFi coprocessor can be sniffed from the TWI2 lines using a logic analyzer
*   See [PIN_MAPPING.md](PIN_MAPPING.md) for TWI2 device address table and additional sensor mappings
