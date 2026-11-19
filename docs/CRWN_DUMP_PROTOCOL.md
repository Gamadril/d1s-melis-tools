# AB5301B ("BLUECRWN") UART bootloader protocol

Reference for the mask-ROM UART download protocol of the Bluetrum AB5301B, the companion MCU that handles sound output on HZ-B500 class F133 boards, and for the RAM stub that reads the MCU's internal flash through it.

Everything here was reverse-engineered from the chip's own mask ROM and from the vendor helper shipped inside `LTTMcu.bin` updates (`helper.bin`, one of the files `mcu_tool extract` writes), cross-checked against kagaimiq's [bluetrum-tools](https://github.com/kagaimiq/bluetrum-tools), and **verified on a real AB5301B on 2026-11-19**: a 1 MiB dump in about 46 s at 921600 baud, decoding to the same bytes that firmware update files carry.

> **Thanks to [kagaimiq/bluetrum-tools](https://github.com/kagaimiq/bluetrum-tools).** That project is where this work started.
>
> What is new here is the CRWN (AB53xx) side: the AB560x "PRAO" download blob does not run on this part, so the RAM stub in §7 was written for it, and the ROM and flash-access details in §6 were dumped from the chip itself.

Byte order notation: **BE** = big-endian, **LE** = little-endian.
Hex values are real values observed on the wire unless marked otherwise.
Register and address values are for CRWN (AB53xx); other Bluetrum families differ.

For the user-facing commands, see [AB5301B.md](AB5301B.md).
The implementation in this tree:

| Piece | Where |
| --- | --- |
| Link and command layers | `mcu_dump/src/proto.rs` |
| Session, flash reads, CLI | `mcu_dump/src/main.rs` |
| RAM stub binary and its C source | `mcu_dump/src/crwn_stub.bin`, `crwn_stub.c` |
| Decoding the resulting dump | `mcu_tool/src/package.rs` (`decode_flash`) |

---

## 1. How a dump works, end to end

1. A 3.3 V UART adapter is attached to the chip's single-wire UART download pin.
2. The host syncs with the mask-ROM bootloader.
3. GET_INFO reports the chip id, the handler load address and a per-session key; AUTHORIZE answers the challenge derived from that key.
4. Optionally the link switches from 115200 to 921600 baud.
5. A 412-byte RAM stub is written to `0x12000` and installed as the command handler.
6. The stub's INIT command returns the code key and the flash JEDEC / unique ids.
7. Repeated stub READ commands stream the flash out.
8. The dump is scrambled; `mcu_tool extract` decodes it into `xcod.bin`, `xres.bin` and `boot.bin`.

The stub exists because the ROM cannot do step 7 itself: `MEM_READ` of the memory-mapped flash window at `0x10000000` **hangs the chip**.

The AB560x ("PRAO") download blob that ships with bluetrum-tools **does not work on CRWN**.
It calls ROM entry points that do not exist here, and its flash access hangs — an endless NAK loop on the first data request is the symptom.

---

## 2. Electrical and serial parameters

| Property | Value |
| --- | --- |
| Framing | 8N1, no flow control |
| Initial baud | 115200 |
| Verified fast baud | 921600 |
| Wiring | single-wire half duplex |
| Download pin | usually `PB3` (shared with USB D+), sometimes `PA7` |

The adapter's **TX and RX are tied together** — the vendor recommendation is a 200 Ω resistor from TX to RX, with RX connected to the chip pin.

The consequence shapes the whole link layer: **every byte the host writes comes back on RX before the chip's answer.** After each write, exactly as many bytes as were written must be read and discarded.
Neither the Python reference nor `mcu_dump` verifies the echo content.

Read timeouts that work in practice: 10 ms per read during sync, 100–300 ms afterwards.

---

## 3. Primitives

### CRC16

CRC-16/CCITT-FALSE: polynomial `0x1021`, initial value `0xFFFF`, not reflected, xorout `0`.
Test vector: `crc16("123456789") = 0x29B1`.

```
crc = init
for b in data:
    crc ^= b << 8
    repeat 8: crc = (crc << 1) ^ 0x1021 if crc & 0x8000 else crc << 1   # u16 wrapping
```

The same function appears throughout the firmware formats, which is why it lives in `mcu_tool/src/codec.rs` and is shared with the dumper.

### Authorization key derivation

```
calckey(k):
    k  = k ^ 0x5555AAAA
    c1 = crc16(k as LE bytes)                 # init 0xFFFF
    c2 = crc16((k ^ 0xFFFFFFFF) as LE bytes)
    return (c1 << 16) | c2
```

| Input | Output |
| --- | --- |
| `0x1B0882C7` | `0x1A1A83D5` |
| `0xDE1447DB` | `0x4C7FD5B0` |
| `0x00000000` | `0x875A1E95` |

---

## 4. Link layer

### 4.1 Sync and reset tokens

These are raw byte sequences, outside the packet framing.

| Name | Bytes | Direction and meaning |
| --- | --- | --- |
| SYNC | `A5 96 87 5A` | host → chip |
| SYNC_RESP | `5A 69 78 A5` | chip → host, when it is in the bootloader |
| RESET | `F5 A0` | communication soft reset; resets the packet counter |
| RESET + SYNC | `F5 A0 A5 96 87 5A` | **resets the chip**, which reboots into the bootloader |

Syncing reliably means alternating plain SYNC attempts with chip resets, because the chip's current baud rate is unknown after a failed session — it may still be at the fast rate from a previous run.
The sequence that works:

```
repeat:
    about ten times: flush input, send SYNC, read 4-byte chunks until one times out
                     a chunk equal to SYNC_RESP means synced
    then once:       send RESET+SYNC at the initial baud, and on the next turn
                     at the target baud, returning the port to the initial baud
```

The **first 4-byte chunk read back is the echo of the host's own SYNC**, so a reader that stops after one chunk never sees the response.
The packet counter is zero after a sync.

### 4.2 Token packets

Every transaction starts with a two-byte token packet, `[token, counter]`.
The counter is a u8 that the host **pre-increments** (wrapping) for each new token packet it sends; the chip answers with `[response_token, same counter]`.

| Token | Value | Meaning |
| --- | --- | --- |
| DATA | `0x4B` | host→chip: data follows. chip→host: the answer to REQUEST, data follows |
| REQUEST | `0xB4` | host→chip: "give me a data packet" |
| PING | `0xC3` | host→chip: "may I send?" |
| ACK | `0x1E` | accepted |
| NAK | `0x2D` | on send: no buffer space (busy). on request: no data yet |
| NYET | `0x3C` | accepted, but the previous packet is not processed yet |

### 4.3 Data payload

```
len: u16 LE | data[len] | crc16(data): u16 LE
```

`len` never exceeds 512.

### 4.4 Sending data (host → chip)

```
payload  = len_le16 + data + crc16(data)_le16
do_ping  = ping_before_send           # sticky state, initially false
loop:
    counter += 1
    packet = [0xC3, counter]  if do_ping  else  [0x4B, counter] + payload
    write packet, consume the echo, read the 2-byte reply
      (on a read timeout, resend the same bytes, up to about ten times)
    if do_ping:  ACK  -> send the data on the next turn
                 NAK  -> keep pinging
    else:        ACK  -> done; clear ping_before_send
                 NYET -> done, accepted; set ping_before_send for the next packet
                 NAK  -> start pinging until ACK, then resend
```

Each new token packet carries a new counter.
A retry after a timeout resends the same bytes, counter included.

### 4.5 Receiving data (chip → host)

```
counter += 1; request = [0xB4, counter]     # built ONCE
loop:
    write request, consume the echo, read the 2-byte reply
      (on a read timeout, resend the same request; give up after about ten)
    DATA (0x4B): read u16 LE len, len bytes, u16 LE crc
                 crc ok -> that is the block; crc bad -> resend the request
    NAK  (0x2D): the chip has nothing yet -> resend the request
```

Resending a REQUEST **with the same counter** makes the chip repeat the same block; a new counter tells it the previous block arrived.
This is why the request is built once and reused for every retry of one block.

The Python reference retries NAK without limit.
An overall timeout is the only protection against a hung handler on the chip, which NAKs forever.

---

## 5. Bootloader commands

### 5.1 Command block

A command block (CB) is 8 bytes, sent as one data packet:

```
cmd: u8 | arg1: u32 BE | arg2: u8 | arg3: u16 BE
```

A command execution consists of the CB, then — for the baud command only — a host-side baud change, then an optional data phase: either N data packets of at most 512 bytes host→chip, or received data packets until the expected total arrives.
**A received packet shorter than requested ends the transfer early**, which is how INIT answers a 48-byte request with 24 bytes.

| Command | Value | arg1 | arg2 | arg3 | Data phase |
| --- | --- | --- | --- | --- | --- |
| IFACE_PARAM | `0x50` | baud | mode | — | receive 2 |
| MEM_READ | `0x52` | address | 0 | blocks | receive blocks × 512 |
| AUTHORIZE | `0x55` | `calckey(commkey)` | 0 | 0 | receive 4 |
| MEM_WRITE | `0x57` | address | 0 | blocks | send blocks × 512 |
| SET_CMD_HANDLER | `0x58` | handler address | 0 | 0 | none |
| GET_INFO | `0x5A` | `0x5259414E` "RYAN" | 0 | `0x67CA` | receive 24 |
| REBOOT | `0x5E` | 0 | 0 | 0 | none |

### 5.2 GET_INFO

CB `5A 52 59 41 4E 00 67 CA`.
The 24-byte answer:

```
chip_id:   [u8; 12]   "BLUECRWN\x01\x00\x00\x00"
load_addr: u32 BE     0x00012000
commkey:   u32 BE     random per session, e.g. 0x1B0882C7
unknown:   u32 BE
```

`load_addr` is where the chip expects a command handler; the stub is linked for that address, so a chip reporting a different one is not supported.

Complete frame with counter 1: `4B 01 08 00 5A 52 59 41 4E 00 67 CA 60 44`

### 5.3 AUTHORIZE

CB `55 | calckey(commkey) BE | 00 | 00 00`, answered with 4 bytes: a new commkey that nothing later uses.
A wrong key is answered with **four zero bytes**, and the commands after it are refused.

For commkey `0x1B0882C7` the CB is `55 1A 1A 83 D5 00 00 00` and the full frame with counter 1 is `4B 01 08 00 55 1A 1A 83 D5 00 00 00 13 32`.

### 5.4 Memory access and handler installation

* **MEM_WRITE** `57 | addr BE | 00 | nblocks BE`, followed by exactly `nblocks` data packets of 512 bytes.
  `arg2 ≠ 0` would scramble the payload with the commkey.
  Example: `57 00 01 20 00 00 00 01` writes one block to `0x12000`.
* **MEM_READ** `52 | addr BE | 00 | nblocks BE`, followed by `nblocks × 512` received bytes.
  It reads back an upload for verification, and the mask ROM itself (`0x80000`–`0x8FFFF`).
  Reading `0x10000000`, the memory-mapped flash, **hangs the chip**.
* **SET_CMD_HANDLER** `58 | addr BE | 00 | 00 00`, no data phase.
  Afterwards every command the ROM does not recognise — `0x00` and `0x01` here — is passed to the handler at that address.
* **REBOOT** `5E 00 00 00 00 00 00 00`, no data phase.

### 5.5 Baud switch

Two commands, both answered with 2 bytes:

1. `50 00 00 00 00 F0 00 00` switches to a faster clock reference.
2. `50 | baud BE | 02 | 00 00` sets the rate; 921600 is `50 00 0E 10 00 02 00 00`.

The host switches its own port **as soon as the second CB packet is ACKed**, then reads the 2-byte response at the new speed.
Skipping both leaves the link at 115200, roughly eight times slower.

---

## 6. Chip internals

Observed by dumping the mask ROM at `0x80000` from the device, and from the vendor helper (`helper.bin` from an `LTTMcu.bin` update extracted with `mcu_tool`, which runs at `0x60000`).

### UART bootloader context, `0x2F810`

| Offset | Contents |
| --- | --- |
| `+0` | transmit buffer pointer |
| `+4` | pointer to the received command block |
| `+60` | arg1, already converted from BE |
| `+64` | arg3 |
| `+68` | custom handler address |
| `+72` | bit 2 set once a handler is installed |

A handler is called as `handler(ctx = 0x2F810)`.

### ROM entry points

| Address | Signature |
| --- | --- |
| `0x80064` | `u32 get_codekey(void)` |
| `0x80068` | `void uart_send(int len)` — sends `len` bytes from `ctx[0]` |
| `0x8006C` | `int uart_recv(void)` |
| `0x8153A` | flash pin setup: SPI0 on GPIOG, `FUNCMCON0 = 0x10`, `SPI0BAUD = 1` |

### Internal flash access

The flash is on-die but is driven like ordinary SPI NOR over the internal SPI0 bus:

| Item | Value |
| --- | --- |
| `SPI0CON` | `0xA8` — bit 16 = transfer done, bit 4 = RX direction |
| `SPI0BUF` | `0xAC` |
| Chip select | GPIOG bit 2 in the data register `0x708`, low = selected |
| Wake | `0xAB` |
| Read | standard SPI NOR read, JEDEC id and unique id |

Write support, which no tool in this tree implements, would use the same path: `06` write enable, `02` page program, `20`/`D8` erase, `05` status polling for busy.

---

## 7. The RAM stub

`mcu_dump/src/crwn_stub.bin`, 412 bytes, sha256 `32dc59551fca70f1a1900b6970dc75f7b044900b7d5385631902a0277c30da54`.
It is linked at `0x12000`, padded with zeros to one 512-byte block for upload, and installed with SET_CMD_HANDLER.
It is **read-only** and lives in RAM only, so a reset removes every trace of it.

### Stub commands

Sent as ordinary command blocks once the handler is installed.

| Command | CB | Answer |
| --- | --- | --- |
| INIT | `00 00 00 00 00 00 00 00` | 24 bytes; requested as 48, the short packet ends the transfer |
| READ | `01 \| addr BE \| 00 \| len BE` | `len` bytes as `ceil(len/512)` packets, `len` = 1..0xFFFF |

INIT sets up the SPI pins and wakes the flash with `0xAB`, so it must precede the first READ.
Its 24-byte answer is all little-endian:

```
codekey: u32
jedec:   u32      = b0 << 16 | b1 << 8 | b2
uid:     [u8; 16]
```

A real answer from the test device:

```
19 04 91 B5 | 14 60 B3 00 | 0B 32 67 30 14 0A 13 24 43 52 43 27 50 20 22 20
```

→ codekey `0xB5910419`, JEDEC `0xB36014`, density byte `0x14`, so the flash size is `1 << 0x14` = 1 MiB.
The density is meaningful for `0x10`–`0x18`.

The codekey is what descrambles the dump's XCOD region.
`mcu_tool` also recovers it on its own from the validated bootstrap, and proves the candidate against every XCOD page CRC, so in practice the value is a cross-check rather than a requirement.

Any chunk size up to `0xFFFF` works for READ.
The Python reference uses `min(0x8000, max(512, align_up(size/100, 512)))`; `mcu_dump` uses the same formula so progress advances in roughly one-percent steps.

### Rebuilding the stub

Source: `mcu_dump/src/crwn_stub.c`.
Linker script:

```
SECTIONS { . = 0x12000; .text : { *(.entry) *(.text*) *(.rodata*) *(.srodata*) *(.data*) *(.sdata*) *(.bss*) *(.sbss*) } }
```

```
riscv64-unknown-elf-gcc -march=rv32imc -mabi=ilp32 -Os -ffreestanding -nostdlib -fno-builtin \
    -T l.ld -Wl,-e,handler -o stub.elf crwn_stub.c
riscv64-unknown-elf-objcopy -O binary stub.elf crwn_stub.bin
```

`handler` has to end up at `0x12000`, which `nm` confirms.
A different gcc produces different bytes; that is expected, and only the entry point placement matters.

---

## 8. A complete session

```
open the port at 115200 8N1
sync                                        # §4.1, counter = 0
GET_INFO                     -> 24 bytes    # chip id starts with "BLUECRWN", load_addr == 0x12000
AUTHORIZE(calckey(commkey))  -> 4 bytes
IFACE_PARAM(arg2 = 0xF0)     -> 2 bytes     # optional baud switch
IFACE_PARAM(baud, arg2 = 2)  -> 2 bytes     # host switches rate after the CB is ACKed
MEM_WRITE(0x12000, blocks)   <- padded stub
MEM_READ(0x12000, blocks)    -> compare with the stub        # optional verification
SET_CMD_HANDLER(0x12000)
INIT                         -> codekey, jedec, uid
for addr in 0 .. 1 << (jedec & 0xFF) step chunk:
    READ(addr, chunk)        -> flash bytes
REBOOT                                       # optional
```

Afterwards the raw image is decoded offline:

```bash
mcu_tool extract ab5301b_flash.bin ab5301b-extracted
```

---

## 9. Failure modes seen during bring-up

| Symptom | Cause |
| --- | --- |
| Sync never succeeds, replies look like the request | The echo is not being consumed; the host reads its own bytes as the answer |
| Endless NAK on REQUEST | The handler on the chip is hung, e.g. the AB560x PRAO blob on a CRWN part. Only an overall timeout breaks out |
| The chip stops answering mid-session | RESET+SYNC (§4.1) recovers it; if that fails, power-cycle the board |
| The serial port disappears | A USB-serial adapter can re-enumerate when the board is power-cycled mid-session |
| Four zero bytes from AUTHORIZE | Wrong key; every later command is refused |
| `MEM_READ` of `0x10000000` never returns | Memory-mapped flash reads hang the ROM — this is what the stub works around |
