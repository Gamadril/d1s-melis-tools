//! SPI NOR over FEL — ported from xfel `spinor.c` + `fel.c` SPI helpers (D1 payload in `d1.rs`).

use crate::d1;
use crate::fel::Fel;
use crate::progress::Progress;
use crate::{Error, Result};

#[repr(u8)]
enum SpiCmd {
  End = 0x00,
  Init = 0x01,
  Select = 0x02,
  Deselect = 0x03,
  Fast = 0x04,
  TxBuf = 0x05,
  RxBuf = 0x06,
  SpinorWait = 0x07,
}

const OPCODE_SFDP: u8 = 0x5a;
const OPCODE_RDID: u8 = 0x9f;
const OPCODE_WRSR: u8 = 0x01;
const OPCODE_WREN: u8 = 0x06;
const OPCODE_READ: u8 = 0x03;
const OPCODE_PROG: u8 = 0x02;
const OPCODE_ENTER_4B: u8 = 0xb7;

#[derive(Clone, Debug)]
struct SpinorInfo {
  name: String,
  id: u32,
  capacity: u64,
  blksz: u32,
  read_granularity: u32,
  write_granularity: u32,
  write_pagesz: u32,
  address_length: u8,
  opcode_read: u8,
  opcode_write: u8,
  opcode_write_enable: u8,
  opcode_erase_4k: u8,
  opcode_erase_32k: u8,
  opcode_erase_64k: u8,
  opcode_erase_256k: u8,
}

struct Pdata {
  info: SpinorInfo,
  swapbuf: u32,
  swaplen: u32,
  cmdlen: u32,
}

pub struct SpinorFlash {
  fel: Fel,
}

#[derive(Clone, Debug)]
pub struct SpinorDetected {
  pub name: String,
  pub capacity: u64,
}

impl SpinorFlash {
  pub fn open() -> Result<Self> {
    let fel = Fel::open()?;
    let mut s = Self { fel };
    if !d1::is_d1(&mut s.fel)? {
      return Err(Error::NotD1);
    }
    Ok(s)
  }

  pub fn version_id(&self) -> u32 {
    self.fel.version.id
  }

  pub fn detect(&mut self) -> Result<SpinorDetected> {
    let p = helper_init(&mut self.fel)?;
    Ok(SpinorDetected {
      name: p.info.name.clone(),
      capacity: p.info.capacity,
    })
  }

  pub fn read(&mut self, addr: u64, buf: &mut [u8]) -> Result<()> {
    let p = helper_init(&mut self.fel)?;
    let total = buf.len() as u64;
    let mut prog = Progress::start(total);
    let mut off = 0usize;
    let mut a = addr;
    let mut rem = total;
    while rem > 0 {
      let n = rem.min(65536) as usize;
      helper_read(&mut self.fel, &p, a as u32, &mut buf[off..off + n]);
      a += n as u64;
      off += n;
      rem -= n as u64;
      if let Some(p) = prog.as_mut() {
        p.update(n as u64);
      }
    }
    if let Some(p) = prog {
      p.stop();
    }
    Ok(())
  }

  pub fn write(&mut self, addr: u64, data: &[u8]) -> Result<()> {
    let p = helper_init(&mut self.fel)?;
    let esize = erase_unit(&p.info).ok_or(Error::Msg("flash has no erase opcode".into()))?;
    let emask = esize as u64 - 1;
    let mut base = addr & !emask;
    let mut cnt = (addr & emask) + data.len() as u64;
    cnt = (cnt + if cnt & emask != 0 { esize as u64 } else { 0 }) & !emask;
    let mut prog = Progress::start(cnt);
    while cnt > 0 {
      let n = cnt.min(262144);
      helper_erase(&mut self.fel, &p, base, n);
      base += n;
      cnt -= n;
      if let Some(p) = prog.as_mut() {
        p.update(n);
      }
    }
    if let Some(p) = prog {
      p.stop();
    }
    let mut prog = Progress::start(data.len() as u64);
    let mut off = 0usize;
    let mut a = addr;
    let mut rem = data.len() as u64;
    while rem > 0 {
      let n = rem.min(65536) as usize;
      helper_write(&mut self.fel, &p, a as u32, &data[off..off + n]);
      a += n as u64;
      off += n;
      rem -= n as u64;
      if let Some(p) = prog.as_mut() {
        p.update(n as u64);
      }
    }
    if let Some(p) = prog {
      p.stop();
    }
    Ok(())
  }

  pub fn erase(&mut self, addr: u64, len: u64) -> Result<()> {
    let p = helper_init(&mut self.fel)?;
    let esize = erase_unit(&p.info).ok_or(Error::Msg("flash has no erase opcode".into()))?;
    let emask = esize as u64 - 1;
    let mut base = addr & !emask;
    let mut cnt = (addr & emask) + len;
    cnt = (cnt + if cnt & emask != 0 { esize as u64 } else { 0 }) & !emask;
    let mut prog = Progress::start(cnt);
    while cnt > 0 {
      let n = cnt.min(262144);
      helper_erase(&mut self.fel, &p, base, n);
      base += n;
      cnt -= n;
      if let Some(p) = prog.as_mut() {
        p.update(n);
      }
    }
    if let Some(p) = prog {
      p.stop();
    }
    Ok(())
  }
}

fn erase_unit(info: &SpinorInfo) -> Option<u32> {
  if info.opcode_erase_4k != 0 {
    Some(4096)
  } else if info.opcode_erase_32k != 0 {
    Some(32768)
  } else if info.opcode_erase_64k != 0 {
    Some(65536)
  } else if info.opcode_erase_256k != 0 {
    Some(262144)
  } else {
    None
  }
}

fn spi_init(fel: &mut Fel) -> Result<Pdata> {
  d1::upload_spi_payload(fel)?;
  let p = Pdata {
    info: SpinorInfo {
      name: String::new(),
      id: 0,
      capacity: 0,
      blksz: 4096,
      read_granularity: 1,
      write_granularity: 256,
      write_pagesz: 256,
      address_length: 3,
      opcode_read: OPCODE_READ,
      opcode_write: OPCODE_PROG,
      opcode_write_enable: OPCODE_WREN,
      opcode_erase_4k: 0,
      opcode_erase_32k: 0,
      opcode_erase_64k: 0,
      opcode_erase_256k: 0,
    },
    swapbuf: d1::SWAP_BUF,
    swaplen: d1::SWAP_LEN,
    cmdlen: d1::CMD_LEN,
  };
  let cbuf = [SpiCmd::Init as u8, SpiCmd::End as u8];
  d1::spi_run(fel, &cbuf)?;
  Ok(p)
}

fn spi_xfer(
  fel: &mut Fel,
  swapbuf: u32,
  swaplen: u32,
  cmdlen: u32,
  tx: Option<&[u8]>,
  rx: Option<&mut [u8]>,
) -> bool {
  let txlen = tx.map(|t| t.len()).unwrap_or(0);
  let rxlen = rx.as_ref().map(|r| r.len()).unwrap_or(0);
  if txlen <= swaplen as usize && rxlen <= swaplen as usize {
    let mut cbuf = Vec::with_capacity(64);
    cbuf.push(SpiCmd::Select as u8);
    if txlen > 0 {
      push_u32_le(&mut cbuf, SpiCmd::TxBuf as u8, swapbuf, txlen as u32);
    }
    if rxlen > 0 {
      push_u32_le(&mut cbuf, SpiCmd::RxBuf as u8, swapbuf, rxlen as u32);
    }
    cbuf.push(SpiCmd::Deselect as u8);
    cbuf.push(SpiCmd::End as u8);
    if let Some(t) = tx {
      let _ = fel.write(swapbuf, t);
    }
    if cbuf.len() as u32 > cmdlen || d1::spi_run(fel, &cbuf).is_err() {
      return false;
    }
    if let Some(r) = rx {
      let _ = fel.read(swapbuf, r);
    }
    true
  } else {
    let cbuf = [SpiCmd::Select as u8, SpiCmd::End as u8];
    if d1::spi_run(fel, &cbuf).is_err() {
      return false;
    }
    let mut tx_off = 0usize;
    let tx = tx.unwrap_or(&[]);
    while tx_off < tx.len() {
      let n = (tx.len() - tx_off).min(swaplen as usize);
      let mut c = Vec::new();
      push_u32_le(&mut c, SpiCmd::TxBuf as u8, swapbuf, n as u32);
      c.push(SpiCmd::End as u8);
      let _ = fel.write(swapbuf, &tx[tx_off..tx_off + n]);
      if d1::spi_run(fel, &c).is_err() {
        return false;
      }
      tx_off += n;
    }
    if let Some(r) = rx {
      let mut rx_off = 0usize;
      while rx_off < r.len() {
        let n = (r.len() - rx_off).min(swaplen as usize);
        let mut c = Vec::new();
        push_u32_le(&mut c, SpiCmd::RxBuf as u8, swapbuf, n as u32);
        c.push(SpiCmd::End as u8);
        if d1::spi_run(fel, &c).is_err() {
          return false;
        }
        let _ = fel.read(swapbuf, &mut r[rx_off..rx_off + n]);
        rx_off += n;
      }
    }
    true
  }
}

fn push_u32_le(cbuf: &mut Vec<u8>, cmd: u8, addr: u32, len: u32) {
  cbuf.push(cmd);
  cbuf.extend_from_slice(&addr.to_le_bytes());
  cbuf.extend_from_slice(&len.to_le_bytes());
}

fn helper_init(fel: &mut Fel) -> Result<Pdata> {
  let mut p = spi_init(fel)?;
  if !spinor_info(fel, &mut p) {
    return Err(Error::NoSpinor);
  }
  run_init_sequence(fel, &p)?;
  Ok(p)
}

fn run_init_sequence(fel: &mut Fel, p: &Pdata) -> Result<()> {
  let mut c = Vec::new();
  let push = |c: &mut Vec<u8>| {
    c.push(SpiCmd::Select as u8);
  };
  let desel = |c: &mut Vec<u8>| c.push(SpiCmd::Deselect as u8);

  push(&mut c);
  c.extend_from_slice(&[SpiCmd::Fast as u8, 2, 0x66, 0x99]);
  desel(&mut c);
  push(&mut c);
  c.push(SpiCmd::SpinorWait as u8);
  desel(&mut c);
  push(&mut c);
  c.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
  desel(&mut c);
  push(&mut c);
  c.extend_from_slice(&[SpiCmd::Fast as u8, 1, 0x98]);
  desel(&mut c);
  push(&mut c);
  c.push(SpiCmd::SpinorWait as u8);
  desel(&mut c);
  push(&mut c);
  c.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
  desel(&mut c);
  push(&mut c);
  c.extend_from_slice(&[SpiCmd::Fast as u8, 2, OPCODE_WRSR, 0]);
  desel(&mut c);
  push(&mut c);
  c.push(SpiCmd::SpinorWait as u8);
  desel(&mut c);

  if p.info.address_length == 4 {
    push(&mut c);
    c.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
    desel(&mut c);
    push(&mut c);
    c.extend_from_slice(&[SpiCmd::Fast as u8, 1, OPCODE_ENTER_4B]);
    desel(&mut c);
    push(&mut c);
    c.push(SpiCmd::SpinorWait as u8);
    desel(&mut c);
  }

  c.push(SpiCmd::End as u8);
  if c.len() as u32 <= p.cmdlen {
    d1::spi_run(fel, &c)?;
    Ok(())
  } else {
    Err(Error::Msg("SPI init command sequence too long".into()))
  }
}

fn spinor_info(fel: &mut Fel, p: &mut Pdata) -> bool {
  if spinor_read_sfdp(fel, p) {
    return true;
  }
  let mut id = 0u32;
  if spinor_read_id(fel, p, &mut id) && id != 0xffffff && id != 0 {
    for t in KNOWN_PARTS {
      if id == t.id {
        p.info = t.to_info();
        return true;
      }
    }
    eprintln!("spi nor id 0x{id:06x} not in built-in table (SFDP failed)");
  }
  false
}

fn spinor_read_id(fel: &mut Fel, p: &Pdata, id: &mut u32) -> bool {
  let tx = [OPCODE_RDID];
  let mut rx = [0u8; 3];
  if !spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx), Some(&mut rx)) {
    return false;
  }
  *id = ((rx[0] as u32) << 16) | ((rx[1] as u32) << 8) | (rx[2] as u32);
  true
}

fn spinor_read_sfdp(fel: &mut Fel, p: &mut Pdata) -> bool {
  const MAX_NPH: usize = 6;
  let mut tx = [0u8; 5];
  let mut hdr = [0u8; 8];
  tx[0] = OPCODE_SFDP;
  if !spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx), Some(&mut hdr)) {
    return false;
  }
  if &hdr[0..4] != b"SFDP" {
    return false;
  }
  // Match xfel: always probe SFDP_MAX_NPH headers (not just hdr.nph).
  let nph = if (hdr[7] as usize) > MAX_NPH {
    hdr[7] as usize + 1
  } else {
    MAX_NPH
  };
  let mut ph = vec![[0u8; 8]; nph];
  for i in 0..nph {
    let addr = i * 8 + 8;
    tx[0] = OPCODE_SFDP;
    tx[1] = ((addr >> 16) & 0xff) as u8;
    tx[2] = ((addr >> 8) & 0xff) as u8;
    tx[3] = (addr & 0xff) as u8;
    tx[4] = 0;
    if !spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx), Some(&mut ph[i])) {
      continue;
    }
  }
  let mut best: Option<SpinorInfo> = None;
  for i in 0..nph {
    if ph[i][0] != 0x00 || ph[i][7] != 0xff {
      continue;
    }
    let addr = (ph[i][4] as u32) | ((ph[i][5] as u32) << 8) | ((ph[i][6] as u32) << 16);
    let len = ph[i][3] as usize * 4;
    if len < 8 {
      continue;
    }
    let mut table = vec![0u8; len];
    tx[0] = OPCODE_SFDP;
    tx[1] = ((addr >> 16) & 0xff) as u8;
    tx[2] = ((addr >> 8) & 0xff) as u8;
    tx[3] = (addr & 0xff) as u8;
    tx[4] = 0;
    if !spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx), Some(&mut table)) {
      continue;
    }
    // ptp==0 reads the SFDP header, not the basic flash parameter table
    if table.len() >= 4 && &table[0..4] == b"SFDP" {
      continue;
    }
    let mut info = p.info.clone();
    apply_sfdp(&mut info, &table, ph[i][2], ph[i][1]);
    // Ignore bogus header / fragment parses (e.g. 16 bytes from misread dword 2)
    if info.capacity < 1024 * 1024 {
      continue;
    }
    if best.as_ref().map(|b| info.capacity > b.capacity).unwrap_or(true) {
      best = Some(info);
    }
  }
  if let Some(info) = best {
    p.info = info;
    return true;
  }
  false
}

/// SFDP basic parameter table dword (1-based), little-endian (JESD216 / xfel `spinor.c`).
fn sfdp_dword(table: &[u8], nth: usize) -> u32 {
  let o = (nth - 1) * 4;
  u32::from_le_bytes(table[o..o + 4].try_into().expect("sfdp table too short"))
}

fn apply_sfdp(info: &mut SpinorInfo, table: &[u8], major: u8, minor: u8) {
  info.name = "SFDP".into();
  info.id = 0;
  // 2nd dword — flash size
  let mut v = sfdp_dword(table, 2);
  info.capacity = if v & (1 << 31) != 0 {
    let exp = (v & 0x7fff_ffff) as u64;
    if exp < 3 {
      0
    } else {
      1u64 << (exp - 3)
    }
  } else {
    ((v as u64) + 1) >> 3
  };
  // 1st dword — address width, erase type in dword 1
  v = sfdp_dword(table, 1);
  info.address_length = if info.capacity <= 16 * 1024 * 1024 && ((v >> 17) & 3) != 2 {
    3
  } else {
    4
  };
  info.opcode_erase_4k = if (v & 3) == 1 { ((v >> 8) & 0xff) as u8 } else { 0 };
  info.opcode_erase_32k = 0;
  info.opcode_erase_64k = 0;
  info.opcode_erase_256k = 0;
  apply_erase_dword(info, sfdp_dword(table, 8));
  apply_erase_dword(info, sfdp_dword(table, 9));
  if info.opcode_erase_4k != 0 {
    info.blksz = 4096;
  } else if info.opcode_erase_32k != 0 {
    info.blksz = 32768;
  } else if info.opcode_erase_64k != 0 {
    info.blksz = 65536;
  } else if info.opcode_erase_256k != 0 {
    info.blksz = 262144;
  }
  info.opcode_write_enable = OPCODE_WREN;
  info.read_granularity = 1;
  info.opcode_read = OPCODE_READ;
  info.write_pagesz = 256;
  if major == 1 && minor < 5 {
    v = sfdp_dword(table, 1);
    info.write_granularity = if (v >> 2) & 1 == 1 { 64 } else { 1 };
  } else if major == 1 && minor >= 5 {
    v = sfdp_dword(table, 11);
    info.write_granularity = 1 << ((v >> 4) & 0xf);
    info.write_pagesz = info.write_granularity;
  }
  info.opcode_write = OPCODE_PROG;
}

fn apply_erase_dword(info: &mut SpinorInfo, v: u32) {
  match v & 0xff {
    12 => info.opcode_erase_4k = ((v >> 8) & 0xff) as u8,
    15 => info.opcode_erase_32k = ((v >> 8) & 0xff) as u8,
    16 => info.opcode_erase_64k = ((v >> 8) & 0xff) as u8,
    18 => info.opcode_erase_256k = ((v >> 8) & 0xff) as u8,
    _ => {}
  }
  match (v >> 16) & 0xff {
    12 => info.opcode_erase_4k = ((v >> 24) & 0xff) as u8,
    15 => info.opcode_erase_32k = ((v >> 24) & 0xff) as u8,
    16 => info.opcode_erase_64k = ((v >> 24) & 0xff) as u8,
    18 => info.opcode_erase_256k = ((v >> 24) & 0xff) as u8,
    _ => {}
  }
}

struct KnownPart {
  name: &'static str,
  id: u32,
  capacity: u64,
  e4k: u8,
  e32k: u8,
  e64k: u8,
}

impl KnownPart {
  fn to_info(&self) -> SpinorInfo {
    SpinorInfo {
      name: self.name.into(),
      id: self.id,
      capacity: self.capacity,
      blksz: 4096,
      read_granularity: 1,
      write_granularity: 256,
      write_pagesz: 256,
      address_length: 3,
      opcode_read: OPCODE_READ,
      opcode_write: OPCODE_PROG,
      opcode_write_enable: OPCODE_WREN,
      opcode_erase_4k: self.e4k,
      opcode_erase_32k: self.e32k,
      opcode_erase_64k: self.e64k,
      opcode_erase_256k: 0,
    }
  }
}

const KNOWN_PARTS: &[KnownPart] = &[
  KnownPart {
    name: "W25X40",
    id: 0xef3013,
    capacity: 512 * 1024,
    e4k: 0x20,
    e32k: 0,
    e64k: 0xd8,
  },
  KnownPart {
    name: "W25Q128JVEIQ",
    id: 0xefc018,
    capacity: 16 * 1024 * 1024,
    e4k: 0x20,
    e32k: 0x52,
    e64k: 0xd8,
  },
  KnownPart {
    name: "GD25D10B",
    id: 0xc84011,
    capacity: 128 * 1024,
    e4k: 0x20,
    e32k: 0x52,
    e64k: 0xd8,
  },
  KnownPart {
    name: "en25qh128",
    id: 0x1c7018,
    capacity: 16 * 1024 * 1024,
    e4k: 0x20,
    e32k: 0,
    e64k: 0xd8,
  },
];

fn helper_read(fel: &mut Fel, p: &Pdata, addr: u32, buf: &mut [u8]) {
  let gran = if p.info.read_granularity == 1 {
    buf.len() as u32
  } else {
    p.info.read_granularity
  };
  let mut pos = 0usize;
  let mut a = addr;
  let mut count = buf.len() as u32;
  while count > 0 {
    let n = count.min(gran) as usize;
    let mut tx = [0u8; 5];
    match p.info.address_length {
      3 => {
        tx[0] = p.info.opcode_read;
        tx[1] = (a >> 16) as u8;
        tx[2] = (a >> 8) as u8;
        tx[3] = a as u8;
        let _ = spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx[..4]), Some(&mut buf[pos..pos + n]));
      }
      4 => {
        tx[0] = p.info.opcode_read;
        tx[1] = (a >> 24) as u8;
        tx[2] = (a >> 16) as u8;
        tx[3] = (a >> 8) as u8;
        tx[4] = a as u8;
        let _ = spi_xfer(fel, p.swapbuf, p.swaplen, p.cmdlen, Some(&tx), Some(&mut buf[pos..pos + n]));
      }
      _ => break,
    }
    a += n as u32;
    pos += n;
    count -= n as u32;
  }
}

fn helper_erase(fel: &mut Fel, p: &Pdata, addr: u64, count: u64) {
  let esize = match erase_unit(&p.info) {
    Some(s) => s as u64,
    None => return,
  };
  let emask = esize - 1;
  let mut base = addr & !emask;
  let mut cnt = (addr & emask) + count;
  cnt = (cnt + if cnt & emask != 0 { esize } else { 0 }) & !emask;
  while cnt > 0 {
    let len = if p.info.opcode_erase_256k != 0 && (base & 0x3_ffff) == 0 && cnt >= 262144 {
      sector_erase(fel, p, base as u32, 256);
      262144
    } else if p.info.opcode_erase_64k != 0 && (base & 0xffff) == 0 && cnt >= 65536 {
      sector_erase(fel, p, base as u32, 64);
      65536
    } else if p.info.opcode_erase_32k != 0 && (base & 0x7fff) == 0 && cnt >= 32768 {
      sector_erase(fel, p, base as u32, 32);
      32768
    } else if p.info.opcode_erase_4k != 0 && (base & 0xfff) == 0 && cnt >= 4096 {
      sector_erase(fel, p, base as u32, 4);
      4096
    } else {
      return;
    };
    base += len;
    cnt -= len;
  }
}

fn sector_erase(fel: &mut Fel, p: &Pdata, addr: u32, k: u32) {
  let (opc, alen) = match k {
    4 => (p.info.opcode_erase_4k, 4u8),
    32 => (p.info.opcode_erase_32k, 4),
    64 => (p.info.opcode_erase_64k, 4),
    256 => (p.info.opcode_erase_256k, 4),
    _ => return,
  };
  let mut c = Vec::new();
  let sel = |c: &mut Vec<u8>| c.push(SpiCmd::Select as u8);
  let des = |c: &mut Vec<u8>| c.push(SpiCmd::Deselect as u8);
  match p.info.address_length {
    3 => {
      sel(&mut c);
      c.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
      des(&mut c);
      sel(&mut c);
      c.extend_from_slice(&[
        SpiCmd::Fast as u8,
        alen,
        opc,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
      ]);
      des(&mut c);
      sel(&mut c);
      c.push(SpiCmd::SpinorWait as u8);
      des(&mut c);
      c.push(SpiCmd::End as u8);
    }
    4 => {
      sel(&mut c);
      c.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
      des(&mut c);
      sel(&mut c);
      c.extend_from_slice(&[
        SpiCmd::Fast as u8,
        alen + 1,
        opc,
        (addr >> 24) as u8,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
      ]);
      des(&mut c);
      sel(&mut c);
      c.push(SpiCmd::SpinorWait as u8);
      des(&mut c);
      c.push(SpiCmd::End as u8);
    }
    _ => return,
  }
  if c.len() as u32 <= p.cmdlen {
    let _ = d1::spi_run(fel, &c);
  }
}

fn helper_write(fel: &mut Fel, p: &Pdata, addr: u32, data: &[u8]) {
  let mut gran = if p.info.write_granularity == 1 {
    data.len() as u32
  } else {
    p.info.write_granularity
  };
  if p.info.write_pagesz > 0 {
    gran = gran.min(p.info.write_pagesz);
  }
  let hdr = match p.info.address_length {
    3 => 4u32,
    4 => 5,
    _ => return,
  };
  gran = gran.min(p.swaplen - hdr);
  let mut pos = 0usize;
  let mut a = addr;
  let mut count = data.len();
  let (cmd_budget, tx_budget) = match p.info.address_length {
    3 => (p.cmdlen as i32 - 19 - 1, p.swaplen as i32 - gran as i32 - 4),
    4 => (p.cmdlen as i32 - 19 - 1, p.swaplen as i32 - gran as i32 - 5),
    _ => return,
  };

  while count > 0 {
    let mut cbuf = Vec::with_capacity(p.cmdlen as usize);
    let mut txbuf = Vec::with_capacity(p.swaplen as usize);
    let mut clen = 0i32;
    let mut txlen = 0i32;
    while clen < cmd_budget && txlen < tx_budget && count > 0 {
      let n = count.min(gran as usize);
      cbuf.push(SpiCmd::Select as u8);
      cbuf.extend_from_slice(&[SpiCmd::Fast as u8, 1, p.info.opcode_write_enable]);
      cbuf.push(SpiCmd::Deselect as u8);
      cbuf.push(SpiCmd::Select as u8);
      let tx_addr = p.swapbuf + txlen as u32;
      push_u32_le(&mut cbuf, SpiCmd::TxBuf as u8, tx_addr, n as u32 + hdr);
      cbuf.push(SpiCmd::Deselect as u8);
      cbuf.push(SpiCmd::Select as u8);
      cbuf.push(SpiCmd::SpinorWait as u8);
      cbuf.push(SpiCmd::Deselect as u8);
      txbuf.push(p.info.opcode_write);
      match p.info.address_length {
        3 => {
          txbuf.push((a >> 16) as u8);
          txbuf.push((a >> 8) as u8);
          txbuf.push(a as u8);
        }
        4 => {
          txbuf.push((a >> 24) as u8);
          txbuf.push((a >> 16) as u8);
          txbuf.push((a >> 8) as u8);
          txbuf.push(a as u8);
        }
        _ => return,
      }
      txbuf.extend_from_slice(&data[pos..pos + n]);
      clen = cbuf.len() as i32;
      txlen = txbuf.len() as i32;
      a += n as u32;
      pos += n;
      count -= n;
    }
    cbuf.push(SpiCmd::End as u8);
    let _ = fel.write(p.swapbuf, &txbuf);
    let _ = d1::spi_run(fel, &cbuf);
  }
}
