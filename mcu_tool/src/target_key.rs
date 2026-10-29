//! Invert the identified bootstrap's CRC-based target-key fingerprint.
//! Build the affine map from the CRC primitive; no customer keys are embedded.

use crate::codec::crc16;
use crate::Result;

pub(crate) fn repeat16(value: u16) -> u32 {
    u32::from(value) | (u32::from(value) << 16)
}

fn fingerprint(key: u32) -> u32 {
    let [b0, b1, b2, b3] = key.to_le_bytes();
    let bytes = [b0, b1, b2, b3, b3, b2, b1, b0];
    (u32::from(crc16(&bytes[..6], 0x5555)) << 16) | u32::from(crc16(&bytes[2..], 0xaaaa))
}

pub(crate) fn recover_keys(target: u32) -> Result<[u32; 2]> {
    let zero = fingerprint(0);
    let columns: [u32; 32] = std::array::from_fn(|j| fingerprint(1 << j) ^ zero);
    // Each u64 contains 32 coefficients and an augmented right-hand bit at 32.
    let mut rows: [u64; 32] = std::array::from_fn(|i| {
        let mut row = u64::from(((target ^ zero) >> i) & 1) << 32;
        for (j, column) in columns.iter().enumerate() {
            row |= u64::from((column >> i) & 1) << j;
        }
        row
    });
    let mut pivots = Vec::new();
    for column in 0..32 {
        let Some(pivot) = (pivots.len()..32).find(|&i| rows[i] & (1 << column) != 0) else {
            continue;
        };
        let rank = pivots.len();
        rows.swap(rank, pivot);
        let row = rows[rank];
        for (i, other) in rows.iter_mut().enumerate() {
            if i != rank && *other & (1 << column) != 0 {
                *other ^= row;
            }
        }
        pivots.push(column);
    }
    if rows.iter().any(|&row| row as u32 == 0 && row >> 32 != 0) {
        return Err(format!(
            "target fingerprint {target:#010x} has no raw-key solution"
        ));
    }
    if pivots.len() != 31 {
        return Err("unsupported target-key fingerprint matrix rank".into());
    }
    let free = (0..32).find(|column| !pivots.contains(column)).unwrap();
    let mut first = 0u32;
    let mut null = 1u32 << free;
    for (row, &pivot) in rows.iter().zip(&pivots) {
        if row & (1 << 32) != 0 {
            first |= 1 << pivot;
        }
        if row & (1 << free) != 0 {
            null |= 1 << pivot;
        }
    }
    Ok([first, first ^ null])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_fingerprint_and_inverse_vectors() {
        assert_eq!(fingerprint(0), 0x05f00be0);
        let mut keys = recover_keys(0x09d4c0cc).unwrap();
        keys.sort();
        assert_eq!(keys, [0x3c990419, 0xb5910419]);
        // The notes' 0x89880000 is a transcription error: these two keys
        // actually differ by 0x89080000, which H maps to the affine zero.
        assert_eq!(keys[0] ^ keys[1], 0x89080000);
    }

    #[test]
    fn other_targets_do_not_reuse_the_reference_key() {
        for key in (0..32).map(|j| 1u32 << j).chain([0, u32::MAX, 0x12345678]) {
            let target = fingerprint(key);
            let candidates = recover_keys(target).unwrap();
            assert!(candidates.contains(&key));
            for candidate in candidates {
                assert_eq!(fingerprint(candidate), target);
            }
        }
    }

    #[test]
    fn inconsistent_fingerprints_are_rejected() {
        // At least one output-bit toggle leaves this rank-31 affine image.
        assert!((0..32).any(|j| recover_keys(0x09d4c0cc ^ (1 << j)).is_err()));
    }
}
