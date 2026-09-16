//! SPI NOR access on Allwinner D1 / F133 via USB FEL (from [xfel](https://github.com/xboot/xfel) spinor + d1_f133).

mod d1;
mod fel;
mod progress;
mod spinor;

pub use fel::Fel;
pub use spinor::SpinorFlash;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("USB: {0}")]
    Usb(#[from] rusb::Error),
    #[error("no Allwinner FEL device (1f3a:efe8)")]
    NoDevice,
    #[error("FEL handshake failed")]
    FelInit,
    #[error("not a D1/F133 (id mismatch)")]
    NotD1,
    #[error("SPI NOR not detected")]
    NoSpinor,
    #[error("{0}")]
    Msg(String),
}
