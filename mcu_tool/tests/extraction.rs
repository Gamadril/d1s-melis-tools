use mcu_tool::codec::{crc16, xor_stream};
use mcu_tool::{decode_wrapper, extract, sha256, Options};

fn put32(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn store_crc(bytes: &mut [u8], offset: usize) {
    let crc = crc16(&bytes[..offset], 0xffff);
    bytes[offset..offset + 2].copy_from_slice(&crc.to_le_bytes());
}

fn wrap(package: &[u8]) -> Vec<u8> {
    let mut input = vec![0; 0x6c];
    put32(&mut input, 0, 0x6c);
    input[4..8].copy_from_slice(&[b'T', 0, b'X', 0]);
    put32(&mut input, 0x64, package.len());
    // Independent forward encoder, rather than the backwards decoding loop.
    let mut acc = 0u8;
    for &plain in package {
        acc = acc.wrapping_add(plain);
        input.push(plain.wrapping_add(acc));
    }
    input[0x68] = acc;
    let checksum = input.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    input.push(checksum);
    input
}

fn update_flash_crcs(package: &mut [u8]) {
    for j in 0..5 {
        let start = 0x1400 + j * 512;
        let crc = crc16(&package[start..start + 512], 0xffff);
        package[0xe00 + 2 * j..0xe02 + 2 * j].copy_from_slice(&crc.to_le_bytes());
    }
}

// Valid relocated sections and loader CRCs, but deliberately unknown boot code.
// Bootstrap identity must be checked before assuming fingerprint semantics.
fn package_fixture() -> Vec<u8> {
    let mut package = vec![0; 0x1f00];
    package[..4].copy_from_slice(b"UPD\x01");
    put32(&mut package, 4, 0x1ef8);
    for (offset, tag, length) in [
        (8, [0xc9, 0xce, 0xc6, 0xcf], 0x14),
        (0x24, [0xc4, 0xc5, 0xd6, 0], 0xc),
        (0x38, [0xc4, 0xc1, 0xd4, 0xc1], 0x4c),
    ] {
        package[offset..offset + 4].copy_from_slice(&tag);
        put32(&mut package, offset + 4, length);
    }
    package[0x10..0x14].copy_from_slice(b"CRWN");
    let data = [
        0x200, 0x800, 0x1400, 0xa00, 0xe00, 0x200, 0x1000, 0x200, 0xa00, 0x200, 0xc00, 0x200, 0,
        0x1200, 0x200, 0, 0x200, 0, 0,
    ];
    for (index, value) in data.iter().enumerate() {
        put32(&mut package, 0x40 + index * 4, *value);
    }
    for (descriptor, start, fill) in [
        (0x240, 0x800, 0xa5),
        (0x250, 0xa00, 0x5a),
        (0x260, 0xc00, 0xff),
    ] {
        let mut plain = [fill; 512];
        let crc = crc16(&plain, 0xffff);
        put32(&mut package, descriptor, start - 0x200);
        put32(&mut package, descriptor + 4, 512);
        put32(&mut package, descriptor + 12, crc as usize);
        if descriptor == 0x260 {
            put32(&mut package, 0x40 + 12 * 4, crc as usize);
        }
        xor_stream(&mut plain, 0x44505558);
        package[start..start + 512].copy_from_slice(&plain);
    }
    let flash = &mut package[0x1400..0x1e00];
    flash[..8].copy_from_slice(b"\x5a\x01\x00\xa5CRWN");
    put32(flash, 0x14, 0x200);
    put32(flash, 0x18, 512);
    let mut boot = [0x55; 512];
    let crc = crc16(&boot, 0xffff);
    flash[0x1c..0x1e].copy_from_slice(&crc.to_le_bytes());
    store_crc(&mut flash[..64], 62);
    xor_stream(&mut flash[..64], 0x474d564c);
    let repeated = u32::from(crc) | (u32::from(crc) << 16);
    xor_stream(&mut boot, repeated ^ 0x474d564c);
    flash[0x200..0x400].copy_from_slice(&boot);
    update_flash_crcs(&mut package);
    store_crc(&mut package[..512], 510);
    package
}

fn error(package: &[u8]) -> String {
    extract(&wrap(package), &Options::default()).unwrap_err()
}

#[test]
fn unknown_bootstrap_is_rejected_but_outer_only_preserves_it() {
    let package = package_fixture();
    assert!(error(&package).contains("unsupported boot-loader variant"));
    let result = extract(&wrap(&package), &Options { outer_only: true }).unwrap();
    assert_eq!(result.wrapper.package, package);
    assert_eq!(result.artifacts.len(), 3);
    assert!(result.parameters.is_none());
    assert!(result
        .manifest()
        .contains("inner package has not been parsed"));
}

#[test]
fn truncated_wrapper_never_panics() {
    let input = wrap(&package_fixture());
    for length in 0..input.len() {
        assert!(decode_wrapper(&input[..length]).is_err(), "length {length}");
    }
}

#[test]
fn checksum_accumulator_and_utf16_are_checked() {
    let mut input = wrap(&package_fixture());
    input[0x6c] ^= 1;
    assert!(decode_wrapper(&input)
        .unwrap_err()
        .contains("additive checksum"));
    let last = input.len() - 1;
    input[last] = input[..last]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    assert!(decode_wrapper(&input).unwrap_err().contains("accumulator"));
    let mut input = wrap(&package_fixture());
    input[4..8].copy_from_slice(&[0, 0xd8, 0, 0]);
    input[last] = input[..last]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    assert!(decode_wrapper(&input).unwrap_err().contains("UTF-16LE"));
}

#[test]
fn package_header_and_record_lengths_are_checked() {
    let mut package = package_fixture();
    package[0x90] ^= 1;
    assert!(error(&package).contains("UPD header CRC16"));
    put32(&mut package, 0x3c, u32::MAX as usize);
    store_crc(&mut package[..512], 510);
    assert!(error(&package).contains("exceeds"));
}

#[test]
fn overlaps_and_huge_offsets_are_rejected() {
    let mut package = package_fixture();
    put32(&mut package, 0x40 + 6 * 4, 0xa00);
    store_crc(&mut package[..512], 510);
    assert!(error(&package).contains("overlapping regions"));
    put32(&mut package, 0x40 + 6 * 4, u32::MAX as usize);
    store_crc(&mut package[..512], 510);
    assert!(error(&package).contains("exceeds"));
}

#[test]
fn loader_descriptors_and_plaintext_crc_are_enforced() {
    let mut package = package_fixture();
    put32(&mut package, 0x250, 0xa00);
    assert!(error(&package).contains("descriptors disagree"));
    let mut package = package_fixture();
    package[0xa00] ^= 1;
    assert!(error(&package).contains("ram.bin CRC16 mismatch"));
}

#[test]
fn encoded_flash_crc_and_partial_pages_are_rejected() {
    let mut package = package_fixture();
    package[0x1400] ^= 1;
    assert!(error(&package).contains("encoded flash page 0 CRC16 mismatch"));
    let mut package = package_fixture();
    put32(&mut package, 0x40 + 3 * 4, 0x9ff);
    store_crc(&mut package[..512], 510);
    assert!(error(&package).contains("whole 512-byte pages"));
}

#[test]
fn boot_crc_and_boot_bounds_are_checked() {
    let mut package = package_fixture();
    package[0x1600] ^= 1;
    update_flash_crcs(&mut package);
    assert!(error(&package).contains("boot loader CRC16 mismatch"));
    let mut package = package_fixture();
    let header = &mut package[0x1400..0x1440];
    xor_stream(header, 0x474d564c);
    put32(header, 0x14, u32::MAX as usize - 511);
    store_crc(header, 62);
    xor_stream(header, 0x474d564c);
    update_flash_crcs(&mut package);
    assert!(error(&package).contains("exceeds"));
}

fn sample(variable: &str) -> Vec<u8> {
    let path =
        std::env::var_os(variable).unwrap_or_else(|| panic!("set {variable} to the sample path"));
    std::fs::read(path).unwrap()
}

fn check_vectors(
    input: &[u8],
    directory: u32,
    vectors: &[(&str, usize, &str)],
    page_counts: &[&str],
) {
    let result = extract(input, &Options::default()).unwrap();
    let parameters = result.parameters.unwrap();
    assert_eq!(parameters.directory, directory);
    assert_eq!(parameters.code, 0x1665b6e5);
    assert_eq!(parameters.raw_key, 0xb5910419);
    assert_eq!(parameters.fingerprint, 0x09d4c0cc);
    assert_eq!(result.wrapper.model, "Ltt5301B");
    for &(path, length, hash) in vectors {
        let file = result
            .artifacts
            .iter()
            .find(|file| file.path == path)
            .unwrap();
        assert_eq!(file.bytes.len(), length, "{path}");
        assert_eq!(sha256(&file.bytes), hash, "{path}");
    }
    let boot = result
        .artifacts
        .iter()
        .find(|file| file.path == "boot.bin")
        .unwrap();
    assert_eq!(
        sha256(&boot.bytes),
        "d0ee4bc492fd580fc7a9b3bbfddf2cd1d32b02c1d99a04686ce66e3932495df0"
    );
    for text in page_counts {
        assert!(result.manifest().contains(text));
    }
    assert!(result.manifest().contains("PASS flash directory CRC16"));
}

#[test]
#[ignore = "requires external reference sample; set LTTMCU_SAMPLE"]
fn reference_sample_matches_independent_vectors_without_parameters() {
    check_vectors(
        &sample("LTTMCU_SAMPLE"),
        0x2f013e09,
        &[
            (
                "package.bin",
                0x91000,
                "f5477318cbc0c53440cc193af2e18d81798836ae46f88bb987e6142fb757f819",
            ),
            (
                "updater.bin",
                0x3400,
                "dcbd513f86827ac6617066a4dc965f9eb262b652988f4ff70099727de02e7570",
            ),
            (
                "ram.bin",
                0x4000,
                "8b7b12726c61ccafe58118b7cd91f0fac8b87a7f464ac5e88c2676154045ff08",
            ),
            (
                "helper.bin",
                0x1000,
                "6117a869f33cd372bfbe2f991be6483db05a7dd22ba288df744c27d632f8c7da",
            ),
            (
                "xcod.bin",
                0x5b000,
                "197f9c771990b77d16faf5cadaaf5bffae5e4f66acf06ec9643d12951ddcc7cf",
            ),
            (
                "xres.bin",
                0x14c00,
                "edf8050e6037116024531bf510ad9cec2c7c1b8d8198cb1751e16aed46a8d345",
            ),
        ],
        &["920/920", "728/728", "166/166"],
    );
}

#[test]
#[ignore = "requires DO_NOT_FLASH sample; set LTTMCU_SECOND_SAMPLE"]
fn second_sample_matches_independent_vectors_without_parameters() {
    check_vectors(
        &sample("LTTMCU_SECOND_SAMPLE"),
        0x0e701f78,
        &[
            (
                "package.bin",
                0x91000,
                "d7fe8c48c0e5249c915e00d97824108dec6f4123264853c1146fa49f832c6097",
            ),
            (
                "updater.bin",
                0x3400,
                "dcbd513f86827ac6617066a4dc965f9eb262b652988f4ff70099727de02e7570",
            ),
            (
                "ram.bin",
                0x4000,
                "8b7b12726c61ccafe58118b7cd91f0fac8b87a7f464ac5e88c2676154045ff08",
            ),
            (
                "helper.bin",
                0x1000,
                "6117a869f33cd372bfbe2f991be6483db05a7dd22ba288df744c27d632f8c7da",
            ),
            (
                "xcod.bin",
                0x5bc00,
                "81763025b8795b43859247b56877d941760fe8900a4340fb4898aa0f88fc1d46",
            ),
            (
                "xres.bin",
                0x14c00,
                "edf8050e6037116024531bf510ad9cec2c7c1b8d8198cb1751e16aed46a8d345",
            ),
        ],
        &["920/920", "734/734", "166/166"],
    );
}

#[test]
#[ignore = "requires Ampire sample; set LTTMCU_AMPIRE_SAMPLE"]
fn ampire_sample_matches_independent_vectors_without_parameters() {
    check_vectors(
        &sample("LTTMCU_AMPIRE_SAMPLE"),
        0xb262a36a,
        &[
            (
                "package.bin",
                0x80800,
                "6e55d6ba8073360da09610e5aa1652111dc571eca75320fb562a210e9b1d4027",
            ),
            (
                "updater.bin",
                0x3c00,
                "2015be0986dff6f0349629b939f2a752ab0bfcebab2a2193c12fc8636d834fb0",
            ),
            (
                "ram.bin",
                0x4000,
                "8b7b12726c61ccafe58118b7cd91f0fac8b87a7f464ac5e88c2676154045ff08",
            ),
            (
                "helper.bin",
                0x1600,
                "ab9ae6adc9f9e5c83f3d750f31205dc87fd20d21cf927f1c8e17f28d318758af",
            ),
            (
                "xcod.bin",
                0x5da00,
                "364c62cf497bdf1dab9f507e7603289d6128898fbce35ed02df2a3e13cd83390",
            ),
            (
                "xres.bin",
                0x14c00,
                "edf8050e6037116024531bf510ad9cec2c7c1b8d8198cb1751e16aed46a8d345",
            ),
        ],
        &["936/936", "749/749", "166/166"],
    );
}

#[test]
fn cli_exposes_no_seed_parameters() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mcu_tool"))
        .args(["extract", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(!help.contains("seed"));
    for removed in [
        "--header-seed",
        "--directory-seed",
        "--code-seed",
        "--resource-seed",
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mcu_tool"))
            .args(["extract", "input.bin", "output", removed, "0"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("unexpected argument"));
    }
}

fn refresh_encoded_flash_table(package: &mut [u8]) {
    let word =
        |offset| u32::from_le_bytes(package[offset..offset + 4].try_into().unwrap()) as usize;
    let (flash, length, table) = (word(0x48), word(0x4c), word(0x50));
    for j in 0..length / 512 {
        let crc = crc16(&package[flash + j * 512..flash + (j + 1) * 512], 0xffff);
        package[table + 2 * j..table + 2 * j + 2].copy_from_slice(&crc.to_le_bytes());
    }
}

fn replace_directory(package: &mut [u8], directory: &[u8]) {
    let flash = u32::from_le_bytes(package[0x48..0x4c].try_into().unwrap()) as usize;
    let crc = crc16(directory, 0xffff);
    let seed = (u32::from(crc) | (u32::from(crc) << 16)) ^ 0x50504158;
    let mut encoded = directory.to_vec();
    xor_stream(&mut encoded, seed);
    package[flash + 0x40..flash + 0x80].copy_from_slice(&encoded);
    package[flash + 0x80..flash + 0x82].copy_from_slice(&crc.to_le_bytes());
    refresh_encoded_flash_table(package);
}

#[test]
#[ignore = "uses reference sample to exercise checksum-correct flag/INFO variations; set LTTMCU_SAMPLE"]
fn image_fields_control_derivation_and_resource_loading() {
    let input = sample("LTTMCU_SAMPLE");
    let original = extract(&input, &Options::default()).unwrap();
    let artifact = |name| {
        original
            .artifacts
            .iter()
            .find(|file| file.path == name)
            .unwrap()
            .bytes
            .clone()
    };
    let package = original.wrapper.package.clone();
    let flash = u32::from_le_bytes(package[0x48..0x4c].try_into().unwrap()) as usize;

    // A nonzero INFO fingerprint takes precedence over the bootstrap's zero default.
    let mut explicit = package.clone();
    put32(&mut explicit, 0x20, 0x09d4c0cc);
    store_crc(&mut explicit[..512], 510);
    assert!(extract(&wrap(&explicit), &Options::default()).is_ok());
    // A different consistent fingerprint must not silently reuse the old code base.
    put32(&mut explicit, 0x20, 0x05f00be0);
    store_crc(&mut explicit[..512], 510);
    assert!(error(&explicit).contains("no target-key candidate"));

    // The directory's independent checksum is enforced after encoded-page CRCs pass.
    let mut damaged = package.clone();
    damaged[flash + 0x80] ^= 1;
    refresh_encoded_flash_table(&mut damaged);
    assert!(error(&damaged).contains("flash directory CRC16"));

    // Repackage resources as plaintext, with directory and encoded-flash CRCs updated.
    let mut plain = package.clone();
    let mut directory = artifact("flash-directory.bin");
    directory[0x1f] = 0;
    let resource_offset = u32::from_le_bytes(directory[16..20].try_into().unwrap()) as usize;
    let prefix = u32::from_le_bytes(
        plain[flash + resource_offset + 4..flash + resource_offset + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let resources = artifact("xres.bin");
    let start = flash + resource_offset + prefix;
    plain[start..start + resources.len()].copy_from_slice(&resources);
    replace_directory(&mut plain, &directory);
    let result = extract(&wrap(&plain), &Options::default()).unwrap();
    assert!(!result.parameters.unwrap().resource_encoded);
    assert_eq!(
        result
            .artifacts
            .iter()
            .find(|file| file.path == "xres.bin")
            .unwrap()
            .bytes,
        resources
    );

    // An absent resource bank is preserved as raw bytes, without claiming decoding.
    let mut absent = package;
    directory[0x1e] = 0;
    replace_directory(&mut absent, &directory);
    let result = extract(&wrap(&absent), &Options::default()).unwrap();
    assert!(!result.parameters.unwrap().resource_present);
    assert!(!result.artifacts.iter().any(|file| file.path == "xres.bin"));
    assert!(result.manifest().contains("SKIP XRES"));
}
