use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// Decode a Melis `apps/Language/*.txt` table and return 原始 → selected language.
///
/// Header: `//	原始	英文	简体中文	...`
/// Rows: `{	开	ON	开	...	}`
/// Default language `"en"` is the `英文` column.
pub fn parse_language_table(bytes: &[u8], lang: &str) -> HashMap<String, String> {
    let text = decode_language_bytes(bytes);
    let mut lines = text.lines();
    let column = lines
        .next()
        .map(|line| language_column(split_fields(line), lang))
        .unwrap_or(2);
    let mut map = HashMap::new();
    for line in text.lines() {
        let fields = split_fields(line);
        if fields.len() <= column || fields[0] != "{" {
            continue;
        }
        let key = clean_field(fields[1]);
        let value = clean_field(fields[column].trim_end_matches('}'));
        if key.is_empty() || value.is_empty() {
            continue;
        }
        map.insert(key, value);
    }
    map
}

/// Load every `*.txt` in `dir` (typically `apps/Language`) for `lang`.
pub fn load_language_dir(dir: impl AsRef<Path>, lang: &str) -> HashMap<String, String> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txt"))
        .collect();
    paths.sort();
    let mut map = HashMap::new();
    for path in paths {
        if let Ok(bytes) = fs::read(&path) {
            map.extend(parse_language_table(&bytes, lang));
        }
    }
    map
}

fn decode_language_bytes(bytes: &[u8]) -> String {
    let utf16 = bytes.len() >= 2
        && (bytes[0] == 0xff && bytes[1] == 0xfe
            || bytes[0] == 0xfe && bytes[1] == 0xff
            || bytes[1] == 0);
    if utf16 {
        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .skip_while(|unit| *unit == 0xfeff)
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn split_fields(line: &str) -> Vec<&str> {
    line.trim_end_matches('\r').split('\t').collect()
}

fn clean_field(field: &str) -> String {
    field.trim().trim_matches('"').trim().to_string()
}

fn language_column(header: Vec<&str>, lang: &str) -> usize {
    let lang = lang.trim();
    if let Some(index) = header.iter().position(|field| field.trim() == lang) {
        return index;
    }
    let aliases: &[&str] = match lang.to_ascii_lowercase().as_str() {
        "en" | "eng" | "english" => &["英文"],
        "zh" | "cn" | "zh-cn" | "zh_cn" | "zh-hans" => &["简体中文"],
        "zh-tw" | "zh_tw" | "zh-hant" => &["繁体中文", "繁體中文"],
        "fr" => &["法语"],
        "de" => &["德语"],
        "ja" => &["日语"],
        "ko" => &["韩语"],
        "ru" => &["俄语"],
        "es" => &["西班牙语"],
        _ => &[],
    };
    header
        .iter()
        .position(|field| aliases.contains(&field.trim()))
        .unwrap_or(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf8_english_column() {
        let src =
            "//\t原始\t英文\t简体中文\r\n{\t开\tON\t开\t}\r\n{\t截图\tscreenshot\t截图\t}\r\n";
        let map = parse_language_table(src.as_bytes(), "en");
        assert_eq!(map.get("开").map(String::as_str), Some("ON"));
        assert_eq!(map.get("截图").map(String::as_str), Some("screenshot"));
    }

    #[test]
    fn dump_setup_txt_english() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Language/Setup.txt";
        let Ok(bytes) = fs::read(path) else {
            return;
        };
        let map = parse_language_table(&bytes, "en");
        assert_eq!(
            map.get("显示内存信息").map(String::as_str),
            Some("Display memory information")
        );
        assert_eq!(map.get("开").map(String::as_str), Some("ON"));
        assert_eq!(
            map.get("主设备").map(String::as_str),
            Some("Main equipment")
        );
        assert_eq!(map.get("截图").map(String::as_str), Some("screenshot"));
    }

    #[test]
    fn load_language_dir_merges_main_and_setup() {
        let dir = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Language";
        if !Path::new(dir).is_dir() {
            return;
        }
        let map = load_language_dir(dir, "en");
        assert_eq!(map.get("手机互联").map(String::as_str), Some("PhoneLink"));
        assert_eq!(
            map.get("显示内存信息").map(String::as_str),
            Some("Display memory information")
        );
    }
}
