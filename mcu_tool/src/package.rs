use std::ops::Range;

use crate::codec::{crc16, xor_stream};
use crate::target_key::{recover_keys, repeat16};
use crate::{
    checked_range, disjoint, sha256, span, u16le, u32le, DecodingParameters, Extraction, Result,
};

const PAGE: usize = 512;
const LOADER_SEED: u32 = 0x44505558;
const HEADER_SEED: u32 = 0x474d564c;
const CACHE_CONSTANT: u32 = 0x50504158;
const DEFAULT_FINGERPRINT: u32 = 0x09d4c0cc;
// Identify the bootstrap whose fingerprint/default/flag behavior was traced.
// This identifies executable semantics, not a complete firmware input/profile.
const BOOTSTRAP_SHA256: &str = "d0ee4bc492fd580fc7a9b3bbfddf2cd1d32b02c1d99a04686ce66e3932495df0";

fn check_crc(name: &str, bytes: &[u8], initial: u16, expected: u16) -> Result<()> {
    let actual = crc16(bytes, initial);
    if actual != expected {
        return Err(format!(
            "{name} CRC16 mismatch: calculated {actual:#06x}, stored {expected:#06x}"
        ));
    }
    Ok(())
}

fn pages(length: usize, name: &str) -> Result<usize> {
    if length == 0 || !length.is_multiple_of(PAGE) {
        return Err(format!(
            "{name}: nonempty whole 512-byte pages required; partial-page policy is unknown"
        ));
    }
    Ok(length / PAGE)
}

fn record<'a>(package: &'a [u8], offset: &mut usize, tag: &[u8; 4]) -> Result<&'a [u8]> {
    // Records live in the first 0x200-byte header, before its CRC field.
    let header = span(package, 0, 0x1fe)?;
    if span(header, *offset, 4)? != tag {
        return Err(format!("unexpected UPD record tag at {:#x}", *offset));
    }
    let length = u32le(header, *offset + 4)? as usize;
    let payload = span(header, *offset + 8, length)?;
    *offset += 8 + length;
    Ok(payload)
}

fn descriptor(package: &[u8], base: usize, relative: usize) -> Result<(Range<usize>, u16)> {
    let start = base
        .checked_add(u32le(package, base + relative)? as usize)
        .ok_or("loader descriptor offset overflow")?;
    let length = u32le(package, base + relative + 4)? as usize;
    Ok((
        checked_range(package, start, length)?,
        u32le(package, base + relative + 12)? as u16,
    ))
}

fn decode_loader(
    package: &[u8],
    range: Range<usize>,
    expected: u16,
    name: &str,
) -> Result<Vec<u8>> {
    let mut decoded = package[range].to_vec();
    pages(decoded.len(), name)?;
    for (j, page) in decoded.as_chunks_mut::<PAGE>().0.iter_mut().enumerate() {
        let index = u32::try_from(j).map_err(|_| "loader page index exceeds u32")?;
        xor_stream(page, LOADER_SEED ^ index);
    }
    check_crc(name, &decoded, 0xffff, expected)?;
    Ok(decoded)
}

#[derive(Debug)]
struct Bank {
    range: Range<usize>,
    prefix: Range<usize>,
    payload: Range<usize>,
    page_count: usize,
    table_offset: usize,
}

fn read_bank(
    flash: &[u8],
    offset: usize,
    directory_length: usize,
    tag: &[u8; 4],
    name: &str,
) -> Result<Bank> {
    let fixed = span(flash, offset, 16)?;
    if &fixed[..4] != tag {
        return Err(format!(
            "{name}: invalid bank tag at flash offset {offset:#x}"
        ));
    }
    check_crc(
        &format!("{name} prefix"),
        &fixed[..14],
        0xffff,
        u16le(fixed, 14)?,
    )?;
    let prefix_length = u32le(fixed, 4)? as usize;
    let length = u32le(fixed, 8)? as usize;
    if directory_length != length {
        return Err(format!(
            "{name}: directory and bank payload lengths disagree"
        ));
    }
    let count = pages(length, name)?;
    if count > u16::MAX as usize {
        return Err(format!(
            "{name}: page index exceeds the supported 16-bit CRC initial value"
        ));
    }
    let table_offset = u16le(fixed, 12)? as usize;
    if table_offset < 16 {
        return Err(format!("{name}: CRC table overlaps the bank header"));
    }
    let prefix = checked_range(flash, offset, prefix_length)?;
    span(&flash[prefix.clone()], table_offset, 2 * count)
        .map_err(|e| format!("{name}: invalid CRC table: {e}"))?;
    let payload = checked_range(flash, prefix.end, length)?;
    Ok(Bank {
        range: prefix.start..payload.end,
        prefix,
        payload,
        page_count: count,
        table_offset,
    })
}

impl Bank {
    fn decode(&self, flash: &[u8], xor_base: Option<u32>, name: &str) -> Result<Vec<u8>> {
        let mut decoded = flash[self.payload.clone()].to_vec();
        for (j, page) in decoded.as_chunks_mut::<PAGE>().0.iter_mut().enumerate() {
            let stored = u16le(&flash[self.prefix.clone()], self.table_offset + 2 * j)?;
            if let Some(base) = xor_base {
                xor_stream(page, base ^ u32::from(stored));
            }
            check_crc(&format!("{name} page {j}"), page, (j + 1) as u16, stored)?;
        }
        Ok(decoded)
    }
}

fn decode_directory(flash: &[u8]) -> Result<(Vec<u8>, u32)> {
    let expected = u16le(flash, 0x80)?;
    let seed = repeat16(expected) ^ CACHE_CONSTANT;
    let mut directory = span(flash, 0x40, 0x40)?.to_vec();
    xor_stream(&mut directory, seed);
    check_crc("flash directory", &directory, 0xffff, expected)?;
    if directory[32..].iter().any(|&byte| byte != 0) {
        return Err("unsupported flash directory: expected two entries and a zero tail".into());
    }
    Ok((directory, seed))
}

struct Boot {
    range: Range<usize>,
    decoded: Vec<u8>,
    seed: u32,
    crc: u16,
    runtime_base: u32,
}

fn decode_boot(flash: &[u8], header: &[u8]) -> Result<Boot> {
    let offset = u32le(header, 0x14)? as usize;
    let length = u32le(header, 0x18)? as usize;
    pages(length, "boot loader")?;
    if offset < PAGE || !offset.is_multiple_of(PAGE) {
        return Err("boot loader offset must be a nonzero 512-byte page boundary".into());
    }
    let range = checked_range(flash, offset, length)?;
    let crc = u16le(header, 0x1c)?;
    let seed = repeat16(crc) ^ HEADER_SEED;
    let mut decoded = flash[range.clone()].to_vec();
    for (j, page) in decoded.as_chunks_mut::<PAGE>().0.iter_mut().enumerate() {
        let index = u32::try_from(offset / PAGE - 1 + j)
            .map_err(|_| "boot loader page index exceeds u32")?;
        xor_stream(page, seed ^ index);
    }
    check_crc("boot loader", &decoded, 0xffff, crc)?;
    if sha256(&decoded) != BOOTSTRAP_SHA256 {
        return Err("unsupported boot-loader variant: fingerprint derivation is not established for these bytes; use --outer-only to preserve the package".into());
    }
    Ok(Boot {
        range,
        decoded,
        seed,
        crc,
        runtime_base: u32le(header, 0xc)?,
    })
}

fn decode_code(
    bank: &Bank,
    flash: &[u8],
    fingerprint: u32,
    boot_crc: u16,
) -> Result<(u32, u32, Vec<u8>)> {
    let mut selected = None;
    for key in recover_keys(fingerprint)? {
        let base = key ^ repeat16(boot_crc) ^ CACHE_CONSTANT;
        if let Ok(decoded) = bank.decode(flash, Some(base), "xcod") {
            if selected.is_some() {
                return Err(
                    "ambiguous target key: both candidates passed every XCOD page CRC".into(),
                );
            }
            selected = Some((key, base, decoded));
        }
    }
    selected.ok_or_else(|| "no target-key candidate passed every XCOD page CRC".into())
}

pub(super) fn decode(package: &[u8], result: &mut Extraction) -> Result<()> {
    if span(package, 0, 4)? != b"UPD\x01" {
        return Err("invalid UPD magic/version".into());
    }
    if u32le(package, 4)? as usize != package.len() - 8 {
        return Err("UPD declared length does not equal package size minus eight".into());
    }
    check_crc(
        "UPD header",
        span(package, 0, 0x1fe)?,
        0xffff,
        u16le(package, 0x1fe)?,
    )?;
    result
        .validations
        .push("PASS UPD magic, length and header CRC16".into());
    let mut offset = 8;
    let info = record(package, &mut offset, b"\xc9\xce\xc6\xcf")?;
    if info.len() != 0x14 || &info[..4] != b"CRWN" {
        return Err("unsupported INFO record".into());
    }
    let stored_fingerprint = u32le(info, 0x10)?;
    let dev = record(package, &mut offset, b"\xc4\xc5\xd6\0")?;
    if dev.len() != 0xc {
        return Err("unsupported DEV record length".into());
    }
    let raw_data = record(package, &mut offset, b"\xc4\xc1\xd4\xc1")?;
    if raw_data.len() != 19 * 4 {
        return Err("unsupported DATA record length (expected nineteen words)".into());
    }
    let mut data = [0usize; 19];
    for (index, value) in data.iter_mut().enumerate() {
        *value = u32le(raw_data, index * 4)? as usize;
    }

    let mut regions = vec![("package header".into(), 0..0x200)];
    for (name, index) in [
        ("updater-section", 0),
        ("main-flash.encoded", 2),
        ("flash-page-crcs", 4),
        ("extra-6", 6),
        ("ram.encoded", 8),
        ("helper.encoded", 10),
        ("extra-13", 13),
    ] {
        if data[index + 1] == 0 {
            return Err(format!("{name}: empty section is unsupported"));
        }
        regions.push((
            name.into(),
            checked_range(package, data[index], data[index + 1])?,
        ));
    }
    disjoint(&regions)?;
    // The descriptors are inside the updater prefix, not arbitrary package bytes.
    let updater = checked_range(package, data[0], data[1])?;
    span(&package[updater.clone()], 0, 0x70)?;
    let (code_range, code_crc) = descriptor(package, data[0], 0x40)?;
    let (ram_range, ram_crc) = descriptor(package, data[0], 0x50)?;
    let (helper_range, helper_crc) = descriptor(package, data[0], 0x60)?;
    if code_range.start < updater.start + 0x70 || code_range.end != updater.end {
        return Err("updater code must follow its descriptor prefix and end at the updater section boundary".into());
    }
    if ram_range != checked_range(package, data[8], data[9])?
        || helper_range != checked_range(package, data[10], data[11])?
        || helper_crc != data[12] as u16
    {
        return Err("RAM/helper descriptors disagree with DATA".into());
    }
    result
        .validations
        .push("PASS section bounds, non-overlap and loader descriptors".into());

    let flash = span(package, data[2], data[3])?;
    let encoded_count = pages(flash.len(), "encoded flash")?;
    let crc_table = span(package, data[4], data[5])?;
    span(crc_table, 0, 2 * encoded_count)?;
    for (j, page) in flash.as_chunks::<PAGE>().0.iter().enumerate() {
        check_crc(
            &format!("encoded flash page {j}"),
            page,
            0xffff,
            u16le(crc_table, 2 * j)?,
        )?;
    }
    result.validations.push(format!(
        "PASS encoded flash CRC16: {encoded_count}/{encoded_count} pages"
    ));

    for (path, range, expected, base) in [
        ("updater.bin", code_range.clone(), code_crc, 0x13000),
        ("ram.bin", ram_range, ram_crc, 0x13000),
        ("helper.bin", helper_range, helper_crc, 0x60000),
    ] {
        let decoded = decode_loader(package, range.clone(), expected, path)?;
        result.add(path, &decoded, "package.bin", range.start, Some(base));
        result
            .validations
            .push(format!("PASS {path} whole-region CRC16 = {expected:#06x}"));
    }

    let mut header = span(flash, 0, 0x40)?.to_vec();
    xor_stream(&mut header, HEADER_SEED);
    if &header[..8] != b"\x5a\x01\x00\xa5CRWN" {
        return Err("decoded flash header signature mismatch".into());
    }
    check_crc(
        "decoded flash header",
        &header[..62],
        0xffff,
        u16le(&header, 62)?,
    )?;
    result
        .validations
        .push("PASS decoded flash header signature and CRC16".into());
    result.add(
        "flash-header.bin",
        &header,
        "raw/main-flash.encoded.bin",
        0,
        None,
    );
    let boot = decode_boot(flash, &header)?;
    result.validations.push(format!(
        "PASS boot loader CRC16 = {:#06x} and identified bootstrap SHA256",
        boot.crc
    ));
    result.add(
        "boot.bin",
        &boot.decoded,
        "package.bin",
        data[2] + boot.range.start,
        Some(boot.runtime_base),
    );
    let (directory, directory_seed) = decode_directory(flash)?;
    result.validations.push(format!(
        "PASS flash directory CRC16 = {:#06x}",
        u16le(flash, 0x80)?
    ));
    let fingerprint = if stored_fingerprint == 0 {
        DEFAULT_FINGERPRINT
    } else {
        stored_fingerprint
    };
    let code = read_bank(
        flash,
        u32le(&directory, 0)? as usize,
        u32le(&directory, 4)? as usize,
        b"\xd8\xc3\xcf\xc4",
        "xcod",
    )?;
    let resource_present = directory[0x1e] != 0;
    let resource_encoded = directory[0x1f] != 0;
    let resource = if resource_present {
        Some(read_bank(
            flash,
            u32le(&directory, 16)? as usize,
            u32le(&directory, 20)? as usize,
            b"\xd8\xd2\xc5\xd3",
            "xres",
        )?)
    } else {
        None
    };
    let mut regions_in_flash = vec![
        ("flash header, directory and CRC".into(), 0..0x82),
        ("boot loader".into(), boot.range.clone()),
        ("xcod".into(), code.range.clone()),
    ];
    if let Some(bank) = &resource {
        regions_in_flash.push(("xres".into(), bank.range.clone()));
    }
    disjoint(&regions_in_flash)?;
    let (raw_key, code_seed, code_bytes) = decode_code(&code, flash, fingerprint, boot.crc)?;
    result.validations.push(
        "PASS target fingerprint inversion: exactly one candidate passed every XCOD page CRC"
            .into(),
    );
    result.parameters = Some(DecodingParameters {
        header: HEADER_SEED,
        directory: directory_seed,
        code: code_seed,
        boot: boot.seed,
        boot_crc: boot.crc,
        fingerprint,
        raw_key,
        resource_present,
        resource_encoded,
    });
    let resource_bytes = resource
        .as_ref()
        .map(|bank| bank.decode(flash, resource_encoded.then_some(0), "xres"))
        .transpose()?;
    let mut decoded_banks = vec![("xcod", &code, code_bytes, 0x10000000)];
    if let Some(bank) = &resource {
        decoded_banks.push(("xres", bank, resource_bytes.unwrap(), 0x11000000));
    } else {
        result
            .validations
            .push("SKIP XRES: directory marks resource bank absent; raw flash is preserved".into());
    }
    let last_bank_end = decoded_banks
        .iter()
        .map(|(_, bank, _, _)| bank.range.end)
        .max()
        .unwrap();
    for (name, bank, bytes, base) in decoded_banks {
        result.add(
            &format!("{name}.bin"),
            &bytes,
            "package.bin",
            data[2] + bank.payload.start,
            Some(base),
        );
        result.add(
            &format!("raw/{name}-prefix.bin"),
            &flash[bank.prefix.clone()],
            "package.bin",
            data[2] + bank.prefix.start,
            None,
        );
        result.validations.push(format!(
            "PASS {name} prefix CRC16 and plaintext page CRC16: {0}/{0} pages",
            bank.page_count
        ));
    }
    result.validations.push(
        "PASS flash directory bounds, bank lengths and non-overlap with bootstrap/header/CRC"
            .into(),
    );
    result.add(
        "flash-directory.bin",
        &directory,
        "raw/main-flash.encoded.bin",
        0x40,
        None,
    );
    result.add(
        "raw/flash-tail.bin",
        &flash[last_bank_end..],
        "package.bin",
        data[2] + last_bank_end,
        None,
    );
    result.add(
        "raw/package-header.bin",
        &package[..0x200],
        "package.bin",
        0,
        None,
    );
    result.add(
        "raw/updater-prefix.bin",
        &package[updater.start..code_range.start],
        "package.bin",
        updater.start,
        None,
    );
    for (name, range) in &regions[1..] {
        result.add(
            &format!("raw/{name}.bin"),
            &package[range.clone()],
            "package.bin",
            range.start,
            None,
        );
    }
    // In the supported layout, everything beyond flash is configuration/schema metadata.
    let flash_end = data[2] + data[3];
    if regions.iter().any(|(_, range)| range.end > flash_end) {
        return Err("unsupported layout: non-flash section extends beyond the flash slice".into());
    }
    result.add(
        "raw/metadata.bin",
        &package[flash_end..],
        "package.bin",
        flash_end,
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(xor_base: Option<u32>) -> Vec<u8> {
        let mut flash = vec![0; 3 * PAGE];
        flash[..4].copy_from_slice(b"\xd8\xc3\xcf\xc4");
        flash[4..8].copy_from_slice(&(PAGE as u32).to_le_bytes());
        flash[8..12].copy_from_slice(&(2 * PAGE as u32).to_le_bytes());
        flash[12..14].copy_from_slice(&16u16.to_le_bytes());
        let crc = crc16(&flash[..14], 0xffff);
        flash[14..16].copy_from_slice(&crc.to_le_bytes());
        for j in 0..2 {
            let mut page = [0x31 + j as u8; PAGE];
            let crc = crc16(&page, (j + 1) as u16);
            flash[16 + 2 * j..18 + 2 * j].copy_from_slice(&crc.to_le_bytes());
            if let Some(base) = xor_base {
                xor_stream(&mut page, base ^ u32::from(crc));
            }
            flash[(j + 1) * PAGE..(j + 2) * PAGE].copy_from_slice(&page);
        }
        flash
    }

    fn bank(flash: &[u8]) -> Result<Bank> {
        read_bank(flash, 0, 2 * PAGE, b"\xd8\xc3\xcf\xc4", "xcod")
    }

    #[test]
    fn candidate_selection_requires_every_page_to_pass() {
        let mut flash = fixture(Some(0x1665b6e5));
        let bank = bank(&flash).unwrap();
        let (key, base, bytes) = decode_code(&bank, &flash, DEFAULT_FINGERPRINT, 0xf3a4).unwrap();
        assert_eq!((key, base), (0xb5910419, 0x1665b6e5));
        assert_eq!(bytes[..PAGE], [0x31; PAGE]);
        assert_eq!(bytes[PAGE..], [0x32; PAGE]);
        flash[2 * PAGE] ^= 1;
        assert!(decode_code(&bank, &flash, DEFAULT_FINGERPRINT, 0xf3a4)
            .unwrap_err()
            .contains("no target-key candidate"));
    }

    #[test]
    fn resource_pages_can_be_plain_or_crc_seeded() {
        for mode in [None, Some(0)] {
            let flash = fixture(mode);
            let decoded = bank(&flash).unwrap().decode(&flash, mode, "xres").unwrap();
            assert_eq!(decoded[..PAGE], [0x31; PAGE]);
            assert_eq!(decoded[PAGE..], [0x32; PAGE]);
            let wrong_mode = if mode.is_some() { None } else { Some(0) };
            assert!(bank(&flash)
                .unwrap()
                .decode(&flash, wrong_mode, "xres")
                .is_err());
        }
    }

    #[test]
    fn bank_header_crc_table_and_bounds_are_enforced() {
        let mut flash = fixture(Some(0));
        flash[14] ^= 1;
        assert!(bank(&flash).unwrap_err().contains("prefix CRC16"));
        flash[12..14].copy_from_slice(&511u16.to_le_bytes());
        let crc = crc16(&flash[..14], 0xffff);
        flash[14..16].copy_from_slice(&crc.to_le_bytes());
        assert!(bank(&flash).unwrap_err().contains("invalid CRC table"));
        let flash = fixture(None);
        assert!(bank(&flash[..flash.len() - 1])
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn directory_seed_comes_from_its_stored_crc() {
        for variant in [0, 1, 0xff] {
            let mut flash = vec![0; 0x82];
            let mut directory = [0; 64];
            directory[0] = variant;
            let crc = crc16(&directory, 0xffff);
            let expected_seed = repeat16(crc) ^ CACHE_CONSTANT;
            let mut encoded = directory;
            xor_stream(&mut encoded, expected_seed);
            flash[0x40..0x80].copy_from_slice(&encoded);
            flash[0x80..0x82].copy_from_slice(&crc.to_le_bytes());
            let (decoded, seed) = decode_directory(&flash).unwrap();
            assert_eq!(decoded, directory);
            assert_eq!(seed, expected_seed);
            flash[0x80] ^= 1;
            assert!(decode_directory(&flash)
                .unwrap_err()
                .contains("directory CRC16"));
        }
    }
}
