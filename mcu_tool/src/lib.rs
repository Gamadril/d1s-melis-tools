//! AB5301B update extraction, independent of Allwinner formats.
//! All decoding and integrity checks finish before the CLI creates output files.

pub mod codec;
mod package;
mod target_key;

use std::fmt::Write as _;
use std::ops::Range;

use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, String>;

/// Parameters derived from the validated bootstrap, INFO and flash directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodingParameters {
    pub header: u32,
    pub directory: u32,
    pub code: u32,
    pub boot: u32,
    pub boot_crc: u16,
    pub fingerprint: u32,
    pub raw_key: u32,
    /// The key came from the caller (`--code-key`) instead of being recovered.
    pub supplied_key: bool,
    pub resource_present: bool,
    pub resource_encoded: bool,
}

#[derive(Default, Debug)]
pub struct Options {
    /// Remove and validate only the outer wrapper; retain the inner bytes intact.
    pub outer_only: bool,
}

#[derive(Debug)]
pub struct Wrapper {
    pub header_size: usize,
    pub model: String,
    pub description: String,
    pub seed: u8,
    pub checksum: u8,
    pub package: Vec<u8>,
}

#[derive(Debug)]
pub struct Artifact {
    /// Fixed tool-controlled relative path, never a filename from the package.
    pub path: String,
    pub bytes: Vec<u8>,
    pub source: String,
    pub offset: usize,
    pub runtime_base: Option<u32>,
}

#[derive(Debug)]
pub struct Extraction {
    /// Absent for a raw flash dump, which has no update-file wrapper.
    pub wrapper: Option<Wrapper>,
    pub input_sha256: String,
    pub parameters: Option<DecodingParameters>,
    pub artifacts: Vec<Artifact>,
    pub validations: Vec<String>,
}

pub fn sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

pub(crate) fn span(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset.checked_add(length).ok_or("offset/length overflow")?;
    data.get(offset..end).ok_or_else(|| {
        format!(
            "range {offset:#x}..{end:#x} exceeds {:#x} bytes",
            data.len()
        )
    })
}

pub(crate) fn u16le(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        span(data, offset, 2)?.try_into().unwrap(),
    ))
}

pub(crate) fn u32le(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        span(data, offset, 4)?.try_into().unwrap(),
    ))
}

pub(crate) fn checked_range(data: &[u8], offset: usize, length: usize) -> Result<Range<usize>> {
    span(data, offset, length)?;
    Ok(offset..offset + length)
}

pub(crate) fn disjoint(regions: &[(String, Range<usize>)]) -> Result<()> {
    let mut sorted = regions.to_vec();
    sorted.sort_by_key(|(_, range)| range.start);
    for pair in sorted.windows(2) {
        if pair[0].1.end > pair[1].1.start {
            return Err(format!(
                "overlapping regions: {} and {}",
                pair[0].0, pair[1].0
            ));
        }
    }
    Ok(())
}

fn utf16_field(bytes: &[u8]) -> Result<String> {
    let words: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|&word| word != 0)
        .collect();
    String::from_utf16(&words).map_err(|e| format!("invalid UTF-16LE wrapper string: {e}"))
}

pub fn decode_wrapper(input: &[u8]) -> Result<Wrapper> {
    let header_size = u32le(input, 0)? as usize;
    if header_size < 0x6c {
        return Err("wrapper header is smaller than 0x6c bytes".into());
    }
    let length = u32le(input, 0x64)? as usize;
    let total = header_size
        .checked_add(length)
        .and_then(|end| end.checked_add(1))
        .ok_or("wrapper length overflow")?;
    if total != input.len() {
        return Err(format!(
            "wrapper length mismatch: declared {total}, actual {}",
            input.len()
        ));
    }
    let checksum = input[..total - 1]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    if checksum != input[total - 1] {
        return Err(format!(
            "outer additive checksum mismatch: calculated {checksum:#04x}, stored {:#04x}",
            input[total - 1]
        ));
    }
    let seed = input[0x68];
    let mut key = seed;
    let mut package = span(input, header_size, length)?.to_vec();
    for byte in package.iter_mut().rev() {
        *byte = byte.wrapping_sub(key);
        key = key.wrapping_sub(*byte);
    }
    if key != 0 {
        return Err(format!(
            "outer decoding accumulator is {key:#04x}, expected zero"
        ));
    }
    Ok(Wrapper {
        header_size,
        model: utf16_field(&input[4..0x34])?,
        description: utf16_field(&input[0x34..0x64])?,
        seed,
        checksum,
        package,
    })
}

/// True when `input` looks like a raw flash image read off the chip rather than
/// an update file: its first 0x40 bytes decode to the CRWN flash header.
pub fn is_flash_image(input: &[u8]) -> bool {
    let Ok(header) = span(input, 0, 8) else {
        return false;
    };
    let mut header = header.to_vec();
    codec::xor_stream(&mut header, package::HEADER_SEED);
    header == b"\x5a\x01\x00\xa5CRWN"
}

/// Decodes a raw flash dump (as produced by `mcu_dump`). `code_key` is the key
/// the chip reports; without it the stock key is recovered from the bootstrap.
pub fn extract_flash(input: &[u8], code_key: Option<u32>) -> Result<Extraction> {
    let mut result = Extraction {
        wrapper: None,
        input_sha256: sha256(input),
        parameters: None,
        artifacts: Vec::new(),
        validations: Vec::new(),
    };
    package::decode_flash(
        input,
        &package::FlashSource {
            name: "input",
            base: 0,
            encoded: "input",
        },
        package::DEFAULT_FINGERPRINT,
        code_key,
        &mut result,
    )?;
    Ok(result)
}

pub fn extract(input: &[u8], options: &Options) -> Result<Extraction> {
    let wrapper = decode_wrapper(input)?;
    let input_sha256 = sha256(input);
    let header_size = wrapper.header_size;
    let package = wrapper.package.clone();
    let mut result = Extraction {
        wrapper: Some(wrapper),
        input_sha256,
        parameters: None,
        artifacts: Vec::new(),
        validations: vec![
            "PASS outer length and additive checksum".into(),
            "PASS outer decoding accumulator == 0".into(),
        ],
    };
    result.add("original.bin", input, "input", 0, None);
    result.add("wrapper-header.bin", &input[..header_size], "input", 0, None);
    result.add("package.bin", &package, "input (outer decoded)", header_size, None);
    if !options.outer_only {
        package::decode(&package, &mut result)?;
    }
    Ok(result)
}

impl Extraction {
    pub(crate) fn add(
        &mut self,
        path: &str,
        bytes: &[u8],
        source: &str,
        offset: usize,
        runtime_base: Option<u32>,
    ) {
        self.artifacts.push(Artifact {
            path: path.into(),
            bytes: bytes.to_vec(),
            source: source.into(),
            offset,
            runtime_base,
        });
    }

    pub fn manifest(&self) -> String {
        let mut out = String::from(
            "mcu_tool manifest v2\nAll offsets below are byte offsets in the named source.\n",
        );
        writeln!(out, "input_sha256 = {}", self.input_sha256).unwrap();
        match &self.wrapper {
            Some(wrapper) => {
                writeln!(
                    out,
                    "input_kind = update file\nmodel = {:?}\ndescription = {:?}",
                    wrapper.model, wrapper.description
                )
                .unwrap();
                writeln!(
                    out,
                    "wrapper_header_size = {:#x}\nwrapper_seed = {:#04x}\nouter_checksum = {:#04x}",
                    wrapper.header_size, wrapper.seed, wrapper.checksum
                )
                .unwrap();
            }
            None => out.push_str("input_kind = raw flash dump\n"),
        }
        if let Some(parameters) = self.parameters {
            // The loaders this seed decodes exist only in an update file.
            if self.wrapper.is_some() {
                out.push_str(
                    "loader_seed = 0x44505558 (XOR zero-based page index, restart per image)\n",
                );
            }
            let (source, key_field) = if parameters.supplied_key {
                ("code key supplied by the caller, validated against every XCOD page CRC16", "supplied_key")
            } else {
                ("automatic derivation from validated bootstrap and image fields", "recovered_key")
            };
            writeln!(out, "mode = full\nparameter_source = {source}\nheader_seed = {:#010x}\ndirectory_seed = {:#010x}\nboot_seed = {:#010x}\nboot_crc16 = {:#06x}\ntarget_fingerprint = {:#010x}\n{key_field} = {:#010x}\ncode_seed = {:#010x}", parameters.header, parameters.directory, parameters.boot, parameters.boot_crc, parameters.fingerprint, parameters.raw_key, parameters.code).unwrap();
            let resource_transform = if !parameters.resource_present {
                "absent (directory presence flag is zero)"
            } else if parameters.resource_encoded {
                "XOR stream seeded by stored page CRC16"
            } else {
                "plaintext"
            };
            writeln!(out, "resource_transform = {resource_transform}").unwrap();
            writeln!(out, "LIMIT: supports the identified AB5301B bootstrap; unknown INFO/directory fields and raw sections are preserved.").unwrap();
            if self.wrapper.is_some() {
                writeln!(out, "LIMIT: runtime bases describe the reference mapping; updater and RAM share a base in separate execution phases.").unwrap();
            }
            writeln!(out, "LIMIT: decoded files are raw images, not a contiguous plaintext flash image; device compatibility is unverified.").unwrap();
        } else {
            writeln!(out, "mode = outer-only\nLIMIT: inner package has not been parsed, decoded or CRC-validated.").unwrap();
        }
        out.push_str("\nValidation\n");
        for check in &self.validations {
            writeln!(out, "{check}").unwrap();
        }
        out.push_str("\nArtifacts\npath\tsource\toffset\tlength\truntime_base\tsha256\n");
        for file in &self.artifacts {
            let runtime = file
                .runtime_base
                .map(|base| format!("{base:#010x}"))
                .unwrap_or_else(|| "-".into());
            writeln!(
                out,
                "{}\t{}\t{:#x}\t{:#x}\t{}\t{}",
                file.path,
                file.source,
                file.offset,
                file.bytes.len(),
                runtime,
                sha256(&file.bytes)
            )
            .unwrap();
        }
        out
    }
}
