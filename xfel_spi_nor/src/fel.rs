use crate::{Error, Result};
use rusb::{DeviceHandle, GlobalContext};
use std::time::Duration;

const VID: u16 = 0x1f3a;
const PID: u16 = 0xefe8;
const TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK: usize = 128 * 1024;

#[repr(C, packed)]
struct UsbRequest {
    magic: [u8; 8],
    length: u32,
    unknown1: u32,
    request: u16,
    length2: u32,
    _pad: [u8; 10],
}

#[repr(C, packed)]
struct FelRequest {
    request: u32,
    address: u32,
    length: u32,
    pad: u32,
}

#[derive(Debug, Clone)]
pub struct FelVersion {
    pub id: u32,
    pub scratchpad: u32,
}

#[derive(Debug)]
pub struct Fel {
    hdl: DeviceHandle<GlobalContext>,
    ep_in: u8,
    ep_out: u8,
    pub version: FelVersion,
}

impl Fel {
    pub fn open() -> Result<Self> {
        let hdl = rusb::open_device_with_vid_pid(VID, PID).ok_or(Error::NoDevice)?;

        if hdl.kernel_driver_active(0).unwrap_or(false) {
            let _ = hdl.detach_kernel_driver(0);
        }
        hdl.claim_interface(0)?;

        let dev = hdl.device();
        let cfg = dev.active_config_descriptor()?;
        let (ep_in, ep_out) = find_bulk_endpoints(&cfg)?;

        let mut fel = Self {
            hdl,
            ep_in,
            ep_out,
            version: FelVersion {
                id: 0,
                scratchpad: 0,
            },
        };
        fel.version = fel.query_version()?;
        Ok(fel)
    }

    fn query_version(&mut self) -> Result<FelVersion> {
        self.fel_request(0x001, 0, 0)?;
        let mut raw = [0u8; 32];
        self.usb_read(&mut raw)?;
        self.read_fel_status()?;
        Ok(FelVersion {
            id: u32::from_le_bytes(raw[8..12].try_into().unwrap()),
            scratchpad: u32::from_le_bytes(raw[20..24].try_into().unwrap()),
        })
    }

    pub fn read32(&mut self, addr: u32) -> Result<u32> {
        let mut v = [0u8; 4];
        self.read(addr, &mut v)?;
        Ok(u32::from_le_bytes(v))
    }

    pub fn write32(&mut self, addr: u32, val: u32) -> Result<()> {
        self.write(addr, &val.to_le_bytes())
    }

    pub fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<()> {
        let mut off = 0usize;
        while off < buf.len() {
            let n = (buf.len() - off).min(65536);
            self.fel_request(0x103, addr + off as u32, n as u32)?;
            self.usb_read(&mut buf[off..off + n])?;
            self.read_fel_status()?;
            off += n;
        }
        Ok(())
    }

    pub fn write(&mut self, addr: u32, buf: &[u8]) -> Result<()> {
        let mut off = 0usize;
        while off < buf.len() {
            let n = (buf.len() - off).min(65536);
            self.fel_request(0x101, addr + off as u32, n as u32)?;
            self.usb_write(&buf[off..off + n])?;
            self.read_fel_status()?;
            off += n;
        }
        Ok(())
    }

    pub fn exec(&mut self, addr: u32) -> Result<()> {
        self.fel_request(0x102, addr, 0)?;
        self.read_fel_status()
    }

    fn fel_request(&mut self, ty: u32, addr: u32, len: u32) -> Result<()> {
        let req = FelRequest {
            request: ty.to_le(),
            address: addr.to_le(),
            length: len.to_le(),
            pad: 0,
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &req as *const FelRequest as *const u8,
                std::mem::size_of::<FelRequest>(),
            )
        };
        self.usb_write(bytes)
    }

    fn usb_write(&mut self, data: &[u8]) -> Result<()> {
        self.send_usb_request(0x12, data.len() as u32)?;
        self.bulk_send(data)?;
        self.read_usb_response()
    }

    fn usb_read(&mut self, data: &mut [u8]) -> Result<()> {
        self.send_usb_request(0x11, data.len() as u32)?;
        self.bulk_recv(data)?;
        self.read_usb_response()
    }

    fn send_usb_request(&mut self, ty: u16, length: u32) -> Result<()> {
        let mut magic = [0u8; 8];
        magic[..4].copy_from_slice(b"AWUC");
        let req = UsbRequest {
            magic,
            length: length.to_le(),
            unknown1: 0x0c00_0000u32.to_le(),
            request: ty.to_le(),
            length2: length.to_le(),
            _pad: [0; 10],
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &req as *const UsbRequest as *const u8,
                std::mem::size_of::<UsbRequest>(),
            )
        };
        self.bulk_send(bytes)
    }

    fn read_usb_response(&mut self) -> Result<()> {
        let mut buf = [0u8; 13];
        self.bulk_recv(&mut buf)?;
        if &buf[..4] != b"AWUS" {
            return Err(Error::Msg("bad AWUS response".into()));
        }
        Ok(())
    }

    fn read_fel_status(&mut self) -> Result<()> {
        let mut buf = [0u8; 8];
        self.usb_read(&mut buf)
    }

    fn bulk_send(&mut self, data: &[u8]) -> Result<()> {
        let mut off = 0;
        while off < data.len() {
            let n = (data.len() - off).min(CHUNK);
            let wrote = self
                .hdl
                .write_bulk(self.ep_out, &data[off..off + n], TIMEOUT)?;
            off += wrote;
        }
        Ok(())
    }

    fn bulk_recv(&mut self, data: &mut [u8]) -> Result<()> {
        let mut off = 0;
        while off < data.len() {
            let got = self.hdl.read_bulk(self.ep_in, &mut data[off..], TIMEOUT)?;
            off += got;
        }
        Ok(())
    }
}

fn find_bulk_endpoints(cfg: &rusb::ConfigDescriptor) -> Result<(u8, u8)> {
    let mut ep_in = None;
    let mut ep_out = None;
    for iface in cfg.interfaces() {
        for setting in iface.descriptors() {
            for ep in setting.endpoint_descriptors() {
                if ep.transfer_type() != rusb::TransferType::Bulk {
                    continue;
                }
                if ep.direction() == rusb::Direction::In {
                    ep_in = Some(ep.address());
                } else {
                    ep_out = Some(ep.address());
                }
            }
        }
    }
    match (ep_in, ep_out) {
        (Some(i), Some(o)) => Ok((i, o)),
        _ => Err(Error::Msg("bulk endpoints not found".into())),
    }
}
