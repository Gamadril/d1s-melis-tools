//! Compiler for Allwinner `sys_config.fex` text files into the binary
//! "script.bin" format (`sys_config.bin` / `melis-config.bin` on Melis).
//!
//! This is a Rust port of `parser_script()` / `_get_str2int()` /
//! `_fill_line_mainkey()` / `_get_item_value()` from Allwinner's original
//! `script.c` (`source/utility/host-tool/script_lichee/script.c` in the
//! lindenis-org Melis 4.0 SDK for the V833, same family as F133/D1s). The
//! binary layout produced here matches that tool byte-for-byte: a
//! `script_head_t` header, a `script_item_t` table (one per `[section]`),
//! a subkey name/offset/type table, and a data region of raw words, all
//! rounded up to the next 1024-byte boundary.
//!
//! Deliberately **not** byte-for-byte identical to the original in one
//! respect: line splitting. The C tool's line scanner only special-cases
//! a bare `\r` (or `;`) as the first byte of a line; a lone `\n` with no
//! preceding `\r` falls through to being treated as the start of a
//! subkey line, which silently merges it with the following line. Files
//! produced by this crate's own `decompile_sys_config` use plain `\n`
//! (including a blank `\n\n` right after the header comment), so a
//! byte-for-bug-compatible port would mis-parse this project's own
//! output. This port treats blank lines and line endings the normal way
//! (`\n` or `\r\n`) instead. Everything that affects the compiled
//! *binary* — value encoding, alignment, offsets, the 1024-byte
//! rounding — is unchanged from the original.

const ITEM_MAIN_NAME_MAX: usize = 32;

const DATA_TYPE_SINGLE_WORD: u32 = 1;
const DATA_TYPE_STRING: u32 = 2;
#[allow(dead_code)]
const DATA_TYPE_MULTI_WORD: u32 = 3; // defined by the original format but never produced by script.c
const DATA_TYPE_GPIO: u32 = 4;
const DATA_EMPTY: u32 = 5;

/// One decoded subkey value, tagged with the same type codes the device's
/// binary config parser understands.
#[derive(Debug, Clone)]
enum Value {
    /// DATA_TYPE_SINGLE_WORD: a plain decimal or `0x` hex integer.
    Word(i32),
    /// DATA_EMPTY: `key =` with nothing after the `=`. Reserves one word
    /// of zeroed data space; matches the original tool exactly.
    Empty,
    /// DATA_TYPE_STRING: already word-aligned, NUL-padded bytes ready to
    /// write straight into the data region.
    Str(Vec<u8>),
    /// DATA_TYPE_GPIO: `port:P<group><pin><mux><pull><drive><data>`.
    /// Always exactly 6 words; missing trailing `<...>` tokens default
    /// to `-1`, same as the original.
    Gpio([i32; 6]),
}

impl Value {
    fn word_len(&self) -> u32 {
        match self {
            Value::Word(_) => 1,
            Value::Empty => 1,
            Value::Str(bytes) => (bytes.len() / 4) as u32,
            Value::Gpio(_) => 6,
        }
    }

    fn type_code(&self) -> u32 {
        match self {
            Value::Word(_) => DATA_TYPE_SINGLE_WORD,
            Value::Empty => DATA_EMPTY,
            Value::Str(_) => DATA_TYPE_STRING,
            Value::Gpio(_) => DATA_TYPE_GPIO,
        }
    }

    /// Data-region words for this value, little-endian, ready to append.
    fn data_words(&self) -> Vec<u8> {
        match self {
            Value::Word(v) => v.to_le_bytes().to_vec(),
            Value::Empty => 0i32.to_le_bytes().to_vec(),
            Value::Str(bytes) => bytes.clone(),
            Value::Gpio(words) => {
                let mut out = Vec::with_capacity(24);
                for w in words {
                    out.extend_from_slice(&w.to_le_bytes());
                }
                out
            }
        }
    }
}

struct Subkey {
    name: String,
    value: Value,
}

struct Section {
    name: String,
    subkeys: Vec<Subkey>,
}

/// Parse a subkey line already split into `name` / raw `value` text (both
/// already trimmed of surrounding whitespace, matching what
/// `_get_item_value` hands to `_get_str2int` in the original).
fn parse_value(raw: &str) -> Result<Value, String> {
    if raw.is_empty() {
        // off == 4 in the original: `key =` with nothing after it.
        return Ok(Value::Empty);
    }

    let bytes = raw.as_bytes();

    // port:P<group><pin><...>  -> GPIO
    if raw.len() >= 6 && raw.starts_with("port:") && (bytes[5] == b'P' || bytes[5] == b'p') {
        return parse_gpio(&raw[6..]);
    }

    // string:<text>  -> string, explicit prefix
    if let Some(rest) = raw.strip_prefix("string:") {
        return Ok(Value::Str(pack_string(rest)));
    }

    // "<text>"  -> quoted string
    if let Some(rest) = raw.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        return Ok(Value::Str(pack_string(&rest[..end])));
    }

    // 0x.../0X...  -> hex integer (always unsigned, matches the original)
    if raw.len() >= 2 && (raw.starts_with("0x") || raw.starts_with("0X")) {
        let digits = &raw[2..];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid hex value: {:?}", raw));
        }
        let v = u32::from_str_radix(digits, 16)
            .map_err(|e| format!("invalid hex value {:?}: {}", raw, e))?;
        return Ok(Value::Word(v as i32));
    }

    // decimal integer (optionally negative)
    let first = bytes[0];
    if first.is_ascii_digit() {
        let v: i64 = raw
            .parse()
            .map_err(|e| format!("invalid decimal value {:?}: {}", raw, e))?;
        return Ok(Value::Word(v as i32));
    }
    if first == b'-' && raw.len() > 1 && bytes[1].is_ascii_digit() {
        let v: i64 = raw
            .parse()
            .map_err(|e| format!("invalid decimal value {:?}: {}", raw, e))?;
        return Ok(Value::Word(v as i32));
    }

    // Fallback: bare, unprefixed string (off == 0 with src == saddr in the original).
    Ok(Value::Str(pack_string(raw)))
}

/// Word-align a string value with a NUL terminator, the same rule the
/// original applies to both quoted and unquoted string values: round the
/// byte length up to a multiple of 4, always leaving room for at least
/// one NUL byte.
fn pack_string(s: &str) -> Vec<u8> {
    let raw = s.as_bytes();
    let padded_len = if raw.len() % 4 == 0 {
        raw.len() + 4
    } else {
        (raw.len() & !0x03) + 4
    };
    let mut out = vec![0u8; padded_len];
    out[..raw.len()].copy_from_slice(raw);
    out
}

/// Parse the remainder of a GPIO value after the `port:P` prefix has
/// already been stripped, e.g. `E03<1><1><default><default>` for
/// `port:PE03<1><1><default><default>`.
fn parse_gpio(rest: &str) -> Result<Value, String> {
    let mut chars = rest.chars().peekable();

    // Group letter, or the literal keyword "ower" (i.e. "Power", with the
    // leading 'P' already consumed as part of the "port:P" prefix).
    let first = chars
        .next()
        .ok_or_else(|| "GPIO value missing group letter".to_string())?;
    let group: i32;
    if (first == 'o' || first == 'O') && rest.len() >= 4 {
        let lower: String = rest.chars().take(4).collect::<String>().to_lowercase();
        if lower == "ower" {
            group = 0xffff; // POWER key
            for _ in 0..3 {
                chars.next();
            }
        } else if first.is_ascii_alphabetic() {
            group = first.to_ascii_uppercase() as i32 - 'A' as i32 + 1;
        } else {
            return Err(format!("invalid GPIO group letter in {:?}", rest));
        }
    } else if first.is_ascii_alphabetic() {
        group = first.to_ascii_uppercase() as i32 - 'A' as i32 + 1;
    } else {
        return Err(format!("invalid GPIO group letter in {:?}", rest));
    }

    // Pin number: decimal digits up to '<'.
    let mut pin_str = String::new();
    while let Some(&c) = chars.peek() {
        if c == '<' {
            break;
        }
        if !c.is_ascii_digit() {
            return Err(format!("invalid GPIO pin number in {:?}", rest));
        }
        pin_str.push(c);
        chars.next();
    }
    let pin: i32 = pin_str
        .parse()
        .map_err(|_| format!("invalid GPIO pin number in {:?}", rest))?;

    // Up to four `<token>` slots: mux, pull, drive, data.
    let mut tokens: Vec<i32> = Vec::with_capacity(4);
    while chars.peek() == Some(&'<') {
        chars.next(); // consume '<'
        let mut tok = String::new();
        loop {
            match chars.next() {
                Some('>') => break,
                Some(c) => tok.push(c.to_ascii_lowercase()),
                None => return Err(format!("unterminated GPIO token in {:?}", rest)),
            }
        }
        let value = match tok.as_str() {
            "default" | "none" | "null" | "-1" => -1,
            _ => tok
                .parse::<i32>()
                .map_err(|_| format!("invalid GPIO token {:?} in {:?}", tok, rest))?,
        };
        tokens.push(value);
        if tokens.len() > 4 {
            return Err(format!("too many GPIO tokens in {:?}", rest));
        }
    }

    if tokens.is_empty() {
        return Err(format!(
            "GPIO value needs at least a mux token: {:?}",
            rest
        ));
    }
    while tokens.len() < 4 {
        tokens.push(-1);
    }

    Ok(Value::Gpio([group, pin, tokens[0], tokens[1], tokens[2], tokens[3]]))
}

/// Split `name = value` on the first `=`, trimming surrounding
/// whitespace/tabs off both sides the way `_get_item_value` does.
fn split_name_value(line: &str) -> Option<(String, String)> {
    let eq = line.find('=')?;
    let name = line[..eq].trim_matches(|c| c == ' ' || c == '\t');
    let value = line[eq + 1..].trim_matches(|c| c == ' ' || c == '\t');
    if name.is_empty() {
        return None;
    }
    // The original truncates subkey names to 31 chars.
    let name: String = name.chars().take(ITEM_MAIN_NAME_MAX - 1).collect();
    Some((name, value.to_string()))
}

fn parse_sections(fex_text: &str) -> Result<Vec<Section>, String> {
    let mut sections: Vec<Section> = Vec::new();

    for raw_line in fex_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start_matches(|c| c == ' ' || c == '\t');

        if trimmed.is_empty() || trimmed.starts_with(';') {
            continue; // blank line or whole-line comment
        }

        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let name: String = name.chars().take(ITEM_MAIN_NAME_MAX - 1).collect();
            sections.push(Section {
                name,
                subkeys: Vec::new(),
            });
            continue;
        }

        // Subkey line. Lines before any [section] header are skipped,
        // same as the original (`if (!new_main_key_flag) break;`).
        let Some(section) = sections.last_mut() else {
            continue;
        };
        let Some((name, raw_value)) = split_name_value(line) else {
            continue;
        };
        let value = parse_value(&raw_value)
            .map_err(|e| format!("in [{}] {} = {:?}: {}", section.name, name, raw_value, e))?;
        section.subkeys.push(Subkey { name, value });
    }

    Ok(sections)
}

fn round_to_1024(n: u32) -> u32 {
    if n % 1024 == 0 {
        n
    } else {
        (n + 1023) / 1024 * 1024
    }
}

/// Compile `sys_config.fex` source text into the compiled binary format
/// (`sys_config.bin` / `melis-config.bin`).
pub fn compile_sys_config(fex_text: &str) -> Result<Vec<u8>, String> {
    let sections = parse_sections(fex_text)?;
    if sections.is_empty() {
        return Err("no [section] found in sys_config.fex".to_string());
    }

    let header_words: u32 = 4; // script_head_t: item_num, length, version[2]
    let item_table_words: u32 = 10 * sections.len() as u32; // script_item_t is 40 bytes = 10 words
    let total_subkeys: u32 = sections.iter().map(|s| s.subkeys.len() as u32).sum();
    let meta_words: u32 = 10 * total_subkeys; // each subkey meta entry is 40 bytes = 10 words

    let meta_region_base = header_words + item_table_words;
    let data_region_base = meta_region_base + meta_words;

    // Pass 1: assign each section's item_offset (word offset of its first
    // subkey meta entry) and each subkey's data offset (word offset into
    // the data region), in the same order the original accumulates them.
    let mut item_table = Vec::with_capacity(sections.len());
    let mut meta_bytes = Vec::new();
    let mut data_bytes = Vec::new();
    let mut running_meta_offset = meta_region_base;
    let mut running_data_offset = data_region_base;

    for section in &sections {
        let mut name_bytes = [0u8; ITEM_MAIN_NAME_MAX];
        let n = section.name.as_bytes();
        let len = n.len().min(ITEM_MAIN_NAME_MAX - 1);
        name_bytes[..len].copy_from_slice(&n[..len]);
        item_table.push((name_bytes, section.subkeys.len() as u32, running_meta_offset));

        for subkey in &section.subkeys {
            let mut sub_name_bytes = [0u8; ITEM_MAIN_NAME_MAX];
            let n = subkey.name.as_bytes();
            let len = n.len().min(ITEM_MAIN_NAME_MAX - 1);
            sub_name_bytes[..len].copy_from_slice(&n[..len]);

            meta_bytes.extend_from_slice(&sub_name_bytes);
            meta_bytes.extend_from_slice(&running_data_offset.to_le_bytes());
            let tag = subkey.value.word_len() | (subkey.value.type_code() << 16);
            meta_bytes.extend_from_slice(&tag.to_le_bytes());

            data_bytes.extend_from_slice(&subkey.value.data_words());
            running_data_offset += subkey.value.word_len();
        }
        running_meta_offset += 10 * section.subkeys.len() as u32;
    }

    let original_len = header_words * 4 + item_table_words * 4 + meta_words * 4
        + (data_bytes.len() as u32);
    let length = round_to_1024(original_len);

    let mut out = Vec::with_capacity(length as usize);
    out.extend_from_slice(&(sections.len() as u32).to_le_bytes()); // item_num
    out.extend_from_slice(&length.to_le_bytes()); // length
    out.extend_from_slice(&1u32.to_le_bytes()); // version[0]
    out.extend_from_slice(&2u32.to_le_bytes()); // version[1]

    for (name_bytes, item_length, item_offset) in &item_table {
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&item_length.to_le_bytes());
        out.extend_from_slice(&item_offset.to_le_bytes());
    }

    out.extend_from_slice(&meta_bytes);
    out.extend_from_slice(&data_bytes);
    out.resize(length as usize, 0);

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_minimal_config() {
        let fex = "\
[uart_para]
uart_debug_port = 0
uart_debug_baudrate = 500000
uart_debug_tx = port:PE02<6><1><default><default>
uart_debug_rx = port:PE03<1><1><default><default>
name_string = string:hello
bare_string = world
quoted = \"a quoted value\"
empty_key =
";
        let bin = compile_sys_config(fex).unwrap();
        assert_eq!(bin.len() % 1024, 0);

        let item_num = u32::from_le_bytes(bin[0..4].try_into().unwrap());
        assert_eq!(item_num, 1);
    }

    #[test]
    fn rejects_empty_input() {
        assert!(compile_sys_config("").is_err());
    }

    #[test]
    fn gpio_defaults_missing_tokens() {
        let fex = "[a]\nkey = port:PA05<3>\n";
        let bin = compile_sys_config(fex).unwrap();
        // header(16) + 1 item_table entry(40) + 1 meta entry(40) = 96, data at word offset 24
        let data_off = 96usize;
        let words: Vec<i32> = (0..6)
            .map(|i| {
                i32::from_le_bytes(
                    bin[data_off + i * 4..data_off + i * 4 + 4]
                        .try_into()
                        .unwrap(),
                )
            })
            .collect();
        assert_eq!(words, vec![1, 5, 3, -1, -1, -1]); // group A=1, pin 5, mux 3, rest default
    }
}
