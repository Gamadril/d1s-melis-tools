//! eGON.BT0 checksum (sun20iw1 / D1s), matches sun20i_d1_spl `gen_check_sum`.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const SEED: u32 = 0x5F0A_6C39;
pub const CHECKSUM_OFFSET: usize = 0x0C;
pub const MAGIC_OFFSET: usize = 0x04;
pub const LENGTH_OFFSET: usize = 0x10;
pub const EXPECTED_MAGIC: &[u8; 8] = b"eGON.BT0";
pub const BLOCK_SIZE: usize = 0x4000;

#[derive(Debug)]
pub enum Error {
  BadMagic { got: [u8; 8] },
  Io(std::io::Error),
}

impl std::fmt::Display for Error {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::BadMagic { got } => write!(
        f,
        "bad magic at 0x{MAGIC_OFFSET:02x}: got {:?}",
        std::str::from_utf8(got).unwrap_or("<non-utf8>")
      ),
      Self::Io(e) => write!(f, "{e}"),
    }
  }
}

impl std::error::Error for Error {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      Self::Io(e) => Some(e),
      _ => None,
    }
  }
}

impl From<std::io::Error> for Error {
  fn from(e: std::io::Error) -> Self {
    Self::Io(e)
  }
}

/// Pad to 16 KiB, patch checksum + length. Returns (checksum word, final size).
pub fn patch_file(path: impl AsRef<Path>) -> Result<(u32, usize), Error> {
  let path = path.as_ref();
  let mut f = OpenOptions::new().read(true).write(true).open(path)?;
  let mut data = Vec::new();
  f.read_to_end(&mut data)?;

  let rem = data.len() % BLOCK_SIZE;
  if rem != 0 {
    data.resize(data.len() + (BLOCK_SIZE - rem), 0);
  }

  let magic: [u8; 8] = data[MAGIC_OFFSET..MAGIC_OFFSET + 8]
    .try_into()
    .expect("slice length");
  if &magic != EXPECTED_MAGIC {
    return Err(Error::BadMagic { got: magic });
  }

  data[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4].copy_from_slice(&SEED.to_le_bytes());

  let total: u32 = data
    .chunks_exact(4)
    .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
    .sum();

  data[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 4].copy_from_slice(&total.to_le_bytes());
  let len = data.len() as u32;
  data[LENGTH_OFFSET..LENGTH_OFFSET + 4].copy_from_slice(&len.to_le_bytes());

  f.seek(SeekFrom::Start(0))?;
  f.set_len(0)?;
  f.write_all(&data)?;

  Ok((total, data.len()))
}
