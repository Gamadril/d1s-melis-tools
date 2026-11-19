//! CRWN UART bootloader link layer (§4) and command layer (§5) of
//! `docs/CRWN_DUMP_PROTOCOL.md`. Single-wire half duplex: every byte written
//! comes back on RX and must be consumed before the reply is read.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use mcu_tool::codec::crc16;
use serialport::{ClearBuffer, SerialPort};

pub type Result<T> = std::result::Result<T, String>;

const SYNC: [u8; 4] = [0xa5, 0x96, 0x87, 0x5a];
const SYNC_RESP: [u8; 4] = [0x5a, 0x69, 0x78, 0xa5];
const RESET: [u8; 2] = [0xf5, 0xa0];

const TOK_DATA: u8 = 0x4b;
const TOK_REQUEST: u8 = 0xb4;
const TOK_PING: u8 = 0xc3;
const TOK_ACK: u8 = 0x1e;
const TOK_NAK: u8 = 0x2d;
const TOK_NYET: u8 = 0x3c;

/// Data packets carry at most 512 bytes; MEM_READ/MEM_WRITE count in blocks.
pub const BLOCK: usize = 512;

const SYNC_TIMEOUT: Duration = Duration::from_millis(10);
const IO_TIMEOUT: Duration = Duration::from_millis(200);
/// Cap on a chip that answers NAK forever (a hung handler never times out).
const STALL_TIMEOUT: Duration = Duration::from_secs(10);
const RETRIES: u32 = 10;

/// Bootloader commands (§5).
pub const IFACE_PARAM: u8 = 0x50;
pub const MEM_READ: u8 = 0x52;
pub const AUTHORIZE: u8 = 0x55;
pub const MEM_WRITE: u8 = 0x57;
pub const SET_CMD_HANDLER: u8 = 0x58;
pub const GET_INFO: u8 = 0x5a;
pub const REBOOT: u8 = 0x5e;
/// Stub commands, dispatched by the handler installed with SET_CMD_HANDLER (§7.1).
pub const STUB_INIT: u8 = 0x00;
pub const STUB_READ: u8 = 0x01;

/// An 8-byte command block: `cmd | arg1 BE | arg2 | arg3 BE` (§5.1).
pub fn command_block(cmd: u8, arg1: u32, arg2: u8, arg3: u16) -> [u8; 8] {
    let a1 = arg1.to_be_bytes();
    let a3 = arg3.to_be_bytes();
    [cmd, a1[0], a1[1], a1[2], a1[3], arg2, a3[0], a3[1]]
}

/// Authorization key derived from the session commkey (§3).
pub fn calckey(key: u32) -> u32 {
    let key = key ^ 0x5555_aaaa;
    let c1 = crc16(&key.to_le_bytes(), 0xffff);
    let c2 = crc16(&(key ^ 0xffff_ffff).to_le_bytes(), 0xffff);
    (u32::from(c1) << 16) | u32::from(c2)
}

/// `len LE | data | crc16(data) LE` (§4.3).
fn data_payload(data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(data.len() + 4);
    payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
    payload.extend_from_slice(data);
    payload.extend_from_slice(&crc16(data, 0xffff).to_le_bytes());
    payload
}

pub struct Link {
    port: Box<dyn SerialPort>,
    counter: u8,
    ping_before_send: bool,
}

impl Link {
    pub fn open(path: &str, baud: u32) -> Result<Self> {
        let port = serialport::new(path, baud)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(IO_TIMEOUT)
            .open()
            .map_err(|e| format!("cannot open {path}: {e}"))?;
        Ok(Self {
            port,
            counter: 0,
            ping_before_send: false,
        })
    }

    fn set_baud(&mut self, baud: u32) -> Result<()> {
        self.port
            .set_baud_rate(baud)
            .map_err(|e| format!("cannot set baud rate {baud}: {e}"))
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<()> {
        self.port
            .set_timeout(timeout)
            .map_err(|e| format!("cannot set port timeout: {e}"))
    }

    /// Reads until `buf` is full; a read timeout mid-packet is an error.
    fn read_exact_timeout(&mut self, buf: &mut [u8]) -> Result<()> {
        let mut got = 0;
        while got < buf.len() {
            match self.port.read(&mut buf[got..]) {
                Ok(0) => return Err("serial port returned no data (disconnected?)".into()),
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(format!("timeout after {got}/{} bytes", buf.len()))
                }
                Err(e) => return Err(format!("serial read failed: {e}")),
            }
        }
        Ok(())
    }

    /// Reads up to `buf.len()` bytes, returning how many arrived before the timeout.
    fn read_upto(&mut self, buf: &mut [u8]) -> usize {
        let mut got = 0;
        while got < buf.len() {
            match self.port.read(&mut buf[got..]) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
                Err(_) => break,
            }
        }
        got
    }

    /// Writes and swallows the half-duplex echo of the same length (§2).
    /// The echo is not verified, matching the reference implementation.
    fn write_echo(&mut self, data: &[u8]) -> Result<()> {
        self.port
            .write_all(data)
            .map_err(|e| format!("serial write failed: {e}"))?;
        self.port
            .flush()
            .map_err(|e| format!("serial flush failed: {e}"))?;
        let mut echo = vec![0u8; data.len()];
        let _ = self.read_exact_timeout(&mut echo);
        Ok(())
    }

    fn token_packet(&mut self, token: u8) -> [u8; 2] {
        self.counter = self.counter.wrapping_add(1);
        [token, self.counter]
    }

    /// Reads a `[token, counter]` reply; a counter mismatch is treated as a
    /// lost packet so the caller resends rather than acting on a stale reply.
    fn recv_token(&mut self) -> Result<u8> {
        let mut reply = [0u8; 2];
        self.read_exact_timeout(&mut reply)?;
        if reply[1] != self.counter {
            return Err(format!(
                "token counter mismatch: got {}, expected {}",
                reply[1], self.counter
            ));
        }
        Ok(reply[0])
    }

    /// Resyncs with the bootloader, resetting the chip if it does not answer (§4.1).
    pub fn sync(&mut self, init_baud: u32, baud: u32) -> Result<()> {
        self.set_timeout(SYNC_TIMEOUT)?;
        let mut reset_at_target = false;
        for attempt in 0..2000 {
            if attempt % 10 == 9 {
                // Reset the chip so it reboots into the bootloader. Its current
                // baud rate is unknown, so alternate between the two candidates.
                let at = if reset_at_target { baud } else { init_baud };
                reset_at_target = !reset_at_target;
                self.set_baud(at)?;
                let mut token = RESET.to_vec();
                token.extend_from_slice(&SYNC);
                self.write_echo(&token)?;
                self.set_baud(init_baud)?;
                continue;
            }
            let _ = self.port.clear(ClearBuffer::Input);
            self.port
                .write_all(&SYNC)
                .map_err(|e| format!("serial write failed: {e}"))?;
            self.port
                .flush()
                .map_err(|e| format!("serial flush failed: {e}"))?;
            // The first chunk read back is the echo of our own SYNC (§4.1).
            loop {
                let mut chunk = [0u8; 4];
                if self.read_upto(&mut chunk) < 4 {
                    break;
                }
                if chunk == SYNC_RESP {
                    self.set_timeout(IO_TIMEOUT)?;
                    self.counter = 0;
                    self.ping_before_send = false;
                    return Ok(());
                }
            }
        }
        Err("no response from the bootloader (check wiring, pin and power)".into())
    }

    /// Sends one data packet, pinging first when the chip asked us to (§4.4).
    fn send_packet(&mut self, data: &[u8]) -> Result<()> {
        let payload = data_payload(data);
        let mut do_ping = self.ping_before_send;
        let start = Instant::now();
        loop {
            if start.elapsed() > STALL_TIMEOUT {
                return Err("chip kept refusing data (handler hung?)".into());
            }
            let mut packet = self
                .token_packet(if do_ping { TOK_PING } else { TOK_DATA })
                .to_vec();
            if !do_ping {
                packet.extend_from_slice(&payload);
            }
            let mut token = None;
            for _ in 0..RETRIES {
                // A retry resends the same bytes, keeping the same counter.
                self.write_echo(&packet)?;
                match self.recv_token() {
                    Ok(value) => {
                        token = Some(value);
                        break;
                    }
                    Err(_) => {
                        let _ = self.port.clear(ClearBuffer::Input);
                    }
                }
            }
            let Some(token) = token else {
                return Err("no reply while sending a data packet".into());
            };
            match (do_ping, token) {
                (true, TOK_ACK) => do_ping = false,
                (true, TOK_NAK) => (),
                (false, TOK_ACK) => {
                    self.ping_before_send = false;
                    return Ok(());
                }
                // Accepted, but the previous packet is still being processed.
                (false, TOK_NYET) => {
                    self.ping_before_send = true;
                    return Ok(());
                }
                (false, TOK_NAK) => do_ping = true,
                (_, other) => return Err(format!("unexpected response token {other:#04x}")),
            }
        }
    }

    /// Requests one data packet (§4.5). The request keeps one counter for all
    /// resends: a new counter would acknowledge the block we failed to read.
    fn recv_packet(&mut self) -> Result<Vec<u8>> {
        let request = self.token_packet(TOK_REQUEST);
        let start = Instant::now();
        let mut tries = 0;
        loop {
            if start.elapsed() > STALL_TIMEOUT {
                return Err("chip never returned data (handler hung?)".into());
            }
            self.write_echo(&request)?;
            let token = match self.recv_token() {
                Ok(token) => {
                    tries = 0;
                    token
                }
                Err(e) => {
                    let _ = self.port.clear(ClearBuffer::Input);
                    tries += 1;
                    if tries > RETRIES {
                        return Err(format!("no reply while requesting a data packet: {e}"));
                    }
                    continue;
                }
            };
            match token {
                TOK_DATA => match self.read_data_payload() {
                    Ok(data) => return Ok(data),
                    // Bad length or CRC: ask for the same block again.
                    Err(_) => {
                        let _ = self.port.clear(ClearBuffer::Input);
                    }
                },
                // No data ready yet.
                TOK_NAK => (),
                other => return Err(format!("unexpected response token {other:#04x}")),
            }
        }
    }

    fn read_data_payload(&mut self) -> Result<Vec<u8>> {
        let mut len = [0u8; 2];
        self.read_exact_timeout(&mut len)?;
        let len = usize::from(u16::from_le_bytes(len));
        if len > BLOCK {
            return Err(format!("data packet too long: {len} bytes"));
        }
        let mut data = vec![0u8; len];
        self.read_exact_timeout(&mut data)?;
        let mut crc = [0u8; 2];
        self.read_exact_timeout(&mut crc)?;
        if crc16(&data, 0xffff) != u16::from_le_bytes(crc) {
            return Err("data packet CRC mismatch".into());
        }
        Ok(data)
    }

    /// Runs one command: command block, optional baud switch, optional data
    /// phase (§5.1). A short received packet ends the transfer early.
    pub fn exec(
        &mut self,
        cb: [u8; 8],
        send: Option<&[u8]>,
        recv: usize,
        switch_baud: Option<u32>,
    ) -> Result<Vec<u8>> {
        self.send_packet(&cb)?;
        if let Some(baud) = switch_baud {
            self.set_baud(baud)?;
        }
        if let Some(send) = send {
            for chunk in send.chunks(BLOCK) {
                self.send_packet(chunk)?;
            }
        }
        let mut data = Vec::with_capacity(recv);
        while data.len() < recv {
            let want = (recv - data.len()).min(BLOCK);
            let block = self.recv_packet()?;
            let short = block.len() != want;
            data.extend_from_slice(&block);
            if short {
                break;
            }
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calckey_vectors() {
        // §3 of the protocol spec.
        assert_eq!(calckey(0x1b08_82c7), 0x1a1a_83d5);
        assert_eq!(calckey(0xde14_47db), 0x4c7f_d5b0);
        assert_eq!(calckey(0), 0x875a_1e95);
    }

    /// Full frames captured from a real session (§5.2, §5.3): token packet with
    /// counter 1, length, command block, CRC.
    #[test]
    fn command_frames_match_capture() {
        let get_info = command_block(GET_INFO, 0x5259_414e, 0, 0x67ca);
        assert_eq!(get_info, [0x5a, 0x52, 0x59, 0x41, 0x4e, 0x00, 0x67, 0xca]);
        let mut frame = vec![TOK_DATA, 1];
        frame.extend_from_slice(&data_payload(&get_info));
        assert_eq!(
            frame,
            [
                0x4b, 0x01, 0x08, 0x00, 0x5a, 0x52, 0x59, 0x41, 0x4e, 0x00, 0x67, 0xca, 0x60, 0x44
            ]
        );

        let auth = command_block(AUTHORIZE, calckey(0x1b08_82c7), 0, 0);
        assert_eq!(auth, [0x55, 0x1a, 0x1a, 0x83, 0xd5, 0x00, 0x00, 0x00]);
        let mut frame = vec![TOK_DATA, 1];
        frame.extend_from_slice(&data_payload(&auth));
        assert_eq!(
            frame,
            [
                0x4b, 0x01, 0x08, 0x00, 0x55, 0x1a, 0x1a, 0x83, 0xd5, 0x00, 0x00, 0x00, 0x13, 0x32
            ]
        );
    }

    /// Half-duplex chip stand-in on the far end of a pty pair: echoes every
    /// byte the host writes, then answers. Queues one reply per command block,
    /// and exercises the NAK/NYET retry paths on the way.
    #[cfg(unix)]
    fn fake_chip(mut port: serialport::TTYPort, info: Vec<u8>, init: Vec<u8>) {
        use std::collections::VecDeque;

        let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
        let mut nak_next_request = false;
        let echo = |port: &mut serialport::TTYPort, n: usize| -> Option<Vec<u8>> {
            let mut buf = vec![0u8; n];
            port.read_exact(&mut buf).ok()?;
            port.write_all(&buf).ok()?;
            port.flush().ok()?;
            Some(buf)
        };
        loop {
            let Some(first) = echo(&mut port, 1) else {
                return;
            };
            match first[0] {
                0xa5 => {
                    echo(&mut port, 3);
                    port.write_all(&SYNC_RESP).unwrap();
                }
                0xf5 => {
                    echo(&mut port, 1);
                }
                TOK_DATA => {
                    let counter = echo(&mut port, 1).unwrap()[0];
                    let len = u16::from_le_bytes(echo(&mut port, 2).unwrap().try_into().unwrap());
                    let data = echo(&mut port, len.into()).unwrap();
                    let crc = u16::from_le_bytes(echo(&mut port, 2).unwrap().try_into().unwrap());
                    assert_eq!(crc16(&data, 0xffff), crc, "host sent a bad CRC");
                    if data.len() == 8 {
                        match data[0] {
                            GET_INFO => pending.push_back(info.clone()),
                            STUB_INIT => pending.push_back(init.clone()),
                            _ => (),
                        }
                    }
                    // NYET on the first command: accepted, ping before the next.
                    let token = if counter == 1 { TOK_NYET } else { TOK_ACK };
                    port.write_all(&[token, counter]).unwrap();
                }
                TOK_PING => {
                    let counter = echo(&mut port, 1).unwrap()[0];
                    port.write_all(&[TOK_ACK, counter]).unwrap();
                }
                TOK_REQUEST => {
                    let counter = echo(&mut port, 1).unwrap()[0];
                    // Make the host re-request once, as a busy chip would.
                    if !nak_next_request {
                        nak_next_request = true;
                        port.write_all(&[TOK_NAK, counter]).unwrap();
                    } else {
                        nak_next_request = false;
                        match pending.pop_front() {
                            Some(data) => {
                                let mut frame = vec![TOK_DATA, counter];
                                frame.extend_from_slice(&data_payload(&data));
                                port.write_all(&frame).unwrap();
                            }
                            None => port.write_all(&[TOK_NAK, counter]).unwrap(),
                        }
                    }
                }
                other => panic!("fake chip got an unknown token {other:#04x}"),
            }
            port.flush().unwrap();
        }
    }

    /// End-to-end over a pty pair: sync, command blocks, data phases, and a
    /// reply shorter than requested ending the transfer early (§5.1).
    #[cfg(unix)]
    #[test]
    fn session_against_a_fake_chip() {
        let (host, chip) = serialport::TTYPort::pair().expect("pty pair");
        let mut info = b"BLUECRWN\x01\x00\x00\x00".to_vec();
        info.extend_from_slice(&0x0001_2000u32.to_be_bytes());
        info.extend_from_slice(&0x1b08_82c7u32.to_be_bytes());
        info.extend_from_slice(&0u32.to_be_bytes());
        let init = {
            let mut init = 0xb591_0419u32.to_le_bytes().to_vec();
            init.extend_from_slice(&0x00b3_6014u32.to_le_bytes());
            init.extend_from_slice(&[0xab; 16]);
            init
        };
        let expected_info = info.clone();
        let expected_init = init.clone();
        let chip = std::thread::spawn(move || fake_chip(chip, info, init));

        let mut link = Link {
            port: Box::new(host),
            counter: 7,
            ping_before_send: false,
        };
        link.set_timeout(IO_TIMEOUT).unwrap();
        link.sync(115_200, 115_200).unwrap();
        assert_eq!(link.counter, 0, "sync resets the packet counter");

        let got = link
            .exec(command_block(GET_INFO, 0x5259_414e, 0, 0x67ca), None, 24, None)
            .unwrap();
        assert_eq!(got, expected_info);

        // A 512-byte data phase, then INIT: 48 bytes asked for, 24 returned.
        link.exec(
            command_block(MEM_WRITE, 0x1_2000, 0, 1),
            Some(&[0x5au8; BLOCK]),
            0,
            None,
        )
        .unwrap();
        let got = link
            .exec(command_block(STUB_INIT, 0, 0, 0), None, 48, None)
            .unwrap();
        assert_eq!(got, expected_init);

        drop(link);
        let _ = chip.join();
    }

    #[test]
    fn mem_write_block_count_is_big_endian() {
        // §5.4: one 512-byte block to $12000.
        assert_eq!(
            command_block(MEM_WRITE, 0x1_2000, 0, 1),
            [0x57, 0x00, 0x01, 0x20, 0x00, 0x00, 0x00, 0x01]
        );
        // §5.5: baud switch to 921600.
        assert_eq!(
            command_block(IFACE_PARAM, 921_600, 0x02, 0),
            [0x50, 0x00, 0x0e, 0x10, 0x00, 0x02, 0x00, 0x00]
        );
    }
}
