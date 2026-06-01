use crate::fel::Fel;
use crate::{Error, Result};

pub const D1_FEL_ID: u32 = 0x0018_5900;
pub const D1_CPU_SIGNATURE: u32 = 0x4301_4281;

pub const PAYLOAD_ADDR: u32 = 0x0002_0000;
pub const CMD_BUF: u32 = 0x0002_1000;
pub const SWAP_BUF: u32 = 0x0002_2000;
pub const SWAP_LEN: u32 = 65536;
pub const CMD_LEN: u32 = 4096;

static SPI_PAYLOAD: &[u8] = include_bytes!("../payloads/d1_spi_init.bin");

pub fn is_d1(fel: &mut Fel) -> Result<bool> {
  if fel.version.id != D1_FEL_ID {
    return Ok(false);
  }
  Ok(fel.read32(0)? == D1_CPU_SIGNATURE)
}

pub fn upload_spi_payload(fel: &mut Fel) -> Result<()> {
  fel.write(PAYLOAD_ADDR, SPI_PAYLOAD)
}

pub fn spi_run(fel: &mut Fel, cbuf: &[u8]) -> Result<()> {
  if cbuf.len() > CMD_LEN as usize {
    return Err(Error::Msg(format!("SPI cmd buffer too large ({} > {})", cbuf.len(), CMD_LEN)));
  }
  fel.write(CMD_BUF, cbuf)?;
  fel.exec(PAYLOAD_ADDR)
}
