//! Integer-only Bluetrum primitives; neither transform is AES or compression.

pub fn crc16(bytes: &[u8], initial: u16) -> u16 {
    let mut crc = initial;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub fn xor_stream(bytes: &mut [u8], mut seed: u32) {
    for byte in bytes {
        *byte ^= seed as u8;
        for _ in 0..8 {
            seed = if seed & 1 != 0 {
                (seed >> 1) ^ 0xa3000000
            } else {
                seed >> 1
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_check_vector() {
        assert_eq!(crc16(b"123456789", 0xffff), 0x29b1);
        assert_eq!(crc16(b"", 0x1234), 0x1234);
    }

    #[test]
    fn stream_outputs_before_advancing() {
        let mut bytes = [0; 4];
        xor_stream(&mut bytes, 1);
        assert_eq!(bytes, [1, 0, 0, 0x46]);
        xor_stream(&mut bytes, 1);
        assert_eq!(bytes, [0; 4]);
    }
}
