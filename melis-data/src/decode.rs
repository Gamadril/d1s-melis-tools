use flate2::read::ZlibDecoder;
use std::io::Read;

use crate::structs::*;

pub fn surface_bytes_per_pixel(format: u32) -> Option<usize> {
    match format {
        1 => Some(4),
        2 => Some(2),
        3 => Some(3),
        _ => None,
    }
}

pub fn decode_raw(method: DecodeMethod, payload: &[u8]) -> Result<&[u8], ParseError> {
    match method {
        DecodeMethod::Raw => Ok(payload),
        other => Err(ParseError::UnsupportedDecodeMethod(other)),
    }
}

pub fn decode_resource(resource: &EmbeddedResource) -> Result<Vec<u8>, ParseError> {
    let raw_payload_offset = resource
        .bytes
        .get(0..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0) as usize;
    let payload_offset = if raw_payload_offset >= 0x18 && raw_payload_offset < resource.bytes.len()
    {
        raw_payload_offset
    } else if resource.bytes.len() >= 0x18 {
        0x18
    } else {
        0
    };
    let payload = resource
        .bytes
        .get(payload_offset..)
        .unwrap_or(resource.bytes.as_slice());
    match resource.decode_method.unwrap_or(DecodeMethod::Raw) {
        DecodeMethod::Raw | DecodeMethod::RowRunLength | DecodeMethod::PaletteRunLength => {
            Ok(payload.to_vec())
        }
        DecodeMethod::Compressed => {
            let mut decoder = ZlibDecoder::new(payload);
            let mut decoded = Vec::new();
            if decoder.read_to_end(&mut decoded).is_ok() && !decoded.is_empty() {
                Ok(decoded)
            } else {
                Ok(payload.to_vec())
            }
        }
        DecodeMethod::Unknown(_) => Ok(payload.to_vec()),
    }
}

fn metadata_cap(serialized_type: u32) -> usize {
    match serialized_type {
        0 => 0x1c4,
        1 | 2 => 0x100,
        3 => 0x108,
        4 => 0xf8,
        5 => 0x1cc,
        _ => 0,
    }
}

fn widget_name(body: &[u8]) -> Option<String> {
    let name = utf16_text(body.get(..body.len().min(200))?);
    if name.is_empty() {
        return None;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
        .then_some(name)
}

fn resource_index_list(body: &[u8], start: usize, count: usize, resource_count: usize) -> Vec<u32> {
    body.get(start..start + count * 4)
        .into_iter()
        .flat_map(|bytes| bytes.chunks_exact(4))
        .map(|word| u32::from_le_bytes(word.try_into().expect("chunk length checked")))
        .filter(|value| *value != u32::MAX && (*value as usize) < resource_count)
        .collect()
}

/// Type-4 image: after metadata, `count` at body+0xe8 of `(kind, resource_index)`
/// pairs. Idle kind is body+0xdc; paint uses the matching pair.
fn type4_resource_indices(
    data: &[u8],
    body: &[u8],
    nested_start: usize,
    resource_count: usize,
) -> Vec<u32> {
    let mut at = nested_start;
    if read_u32(body, 0xe0).unwrap_or(0) != 0 {
        at = at.saturating_add(8);
    }
    if read_u32(body, 0xe4).unwrap_or(0) != 0 {
        at = at.saturating_add(8);
    }
    let count = read_u32(body, 0xe8).unwrap_or(0) as usize;
    let idle_kind = read_u32(body, 0xdc).unwrap_or(0);
    let mut idle = Vec::new();
    let mut rest = Vec::new();
    for _ in 0..count {
        let kind = read_u32(data, at).unwrap_or(0);
        let index = read_u32(data, at.saturating_add(4)).unwrap_or(u32::MAX);
        at = at.saturating_add(8);
        if index == u32::MAX || (index as usize) >= resource_count {
            continue;
        }
        if kind == idle_kind {
            idle.push(index);
        } else {
            rest.push(index);
        }
    }
    idle.extend(rest);
    idle
}

fn take_child(
    data: &[u8],
    cursor: &mut usize,
    block_end: usize,
    expected: Option<u32>,
    screen_width: u32,
    screen_height: u32,
    resource_count: usize,
    children: &mut Vec<UiObject>,
) {
    if *cursor + 8 > block_end {
        return;
    }
    let child_type = read_u32(data, *cursor).unwrap_or(u32::MAX);
    let child_offset = read_u32(data, *cursor + 4).unwrap_or(0) as usize;
    *cursor += 8;
    let type_ok = match expected {
        Some(expected_type) => child_type == expected_type,
        None => child_type <= 4,
    };
    if type_ok && child_offset >= 8 && child_offset < data.len() {
        children.push(decode_ui_block(
            children.len(),
            child_type,
            child_offset,
            data,
            screen_width,
            screen_height,
            resource_count,
        ));
    }
}

pub fn decode_ui_block(
    index: usize,
    serialized_type: u32,
    block_offset: usize,
    data: &[u8],
    screen_width: u32,
    screen_height: u32,
    resource_count: usize,
) -> UiObject {
    let total_size = read_u32(data, block_offset).unwrap_or(0) as usize;
    let file_meta = read_u32(data, block_offset + 4).unwrap_or(0) as usize;
    let metadata_size = file_meta.min(metadata_cap(serialized_type));
    let body = data
        .get(block_offset + 8..block_offset + 8 + metadata_size)
        .unwrap_or_default();
    let rect_offset = if serialized_type == 0 { 0x194 } else { 0xcc };
    let bounds = read_rect(body, rect_offset).filter(|rect| {
        // Keep off-screen page widgets (MainApp x=1084/1274). Firmware clips;
        // dropping the rect made the renderer stretch them to the full canvas.
        rect.width > 0
            && rect.height > 0
            && rect.width <= screen_width
            && rect.height <= screen_height
    });
    let elements = bounds.into_iter().collect::<Vec<_>>();
    let name = widget_name(body);
    let string_id = match serialized_type {
        1 | 2 => read_u32(body, 0xf0)
            .ok()
            .filter(|&id| id != 0 && id != u32::MAX),
        _ => None,
    };
    let text = (serialized_type == 0)
        .then(|| body.get(0xcc..0x194).map(utf16_text))
        .flatten()
        .filter(|text| !text.is_empty());
    let text_color = (serialized_type == 0)
        .then(|| read_u32(body, 0x1a8).ok())
        .flatten()
        .filter(|&color| color != 0 && color != u32::MAX)
        .map(|color| {
            (
                ((color >> 16) & 0xff) as u8,
                ((color >> 8) & 0xff) as u8,
                (color & 0xff) as u8,
            )
        });
    let text_align = (serialized_type == 0).then(|| match read_u32(body, 0x1c0).unwrap_or(0) & 3 {
        0 => TextAlign::Left,
        2 => TextAlign::Right,
        _ => TextAlign::Center,
    });
    let mut resource_indices = match serialized_type {
        // Type 1: four idle/press/… state indices at +0xdc.
        // Type 2: surface list at +0xdc (terminator 0xffffffff).
        1 | 2 => resource_index_list(body, 0xdc, 4, resource_count),
        // Type 5: view bitmap index at +0xdc.
        5 => resource_index_list(body, 0xdc, 1, resource_count),
        _ => Vec::new(),
    };
    let grid = (serialized_type == 3)
        .then(|| UiGrid {
            columns: read_u32(body, 0xdc).unwrap_or(0),
            rows: read_u32(body, 0xe0).unwrap_or(0),
            cell_width: read_u32(body, 0xe4).unwrap_or(0),
            cell_height: read_u32(body, 0xe8).unwrap_or(0),
        })
        .filter(|grid| grid.columns > 0 && grid.rows > 0);
    let nested_start = block_offset.saturating_add(8).saturating_add(file_meta);
    if serialized_type == 4 {
        resource_indices = type4_resource_indices(data, body, nested_start, resource_count);
    }
    let block_end = block_offset.saturating_add(total_size).min(data.len());
    let mut children = Vec::new();
    let mut cursor = nested_start;
    match serialized_type {
        1 => {
            if read_u32(body, 0xfc).unwrap_or(0) != 0 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    Some(4),
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
            if read_u32(body, 0xf4).unwrap_or(0) != 0 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    Some(0),
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
        }
        2 => {
            if read_u32(body, 0xf8).unwrap_or(0) != 0 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    Some(0),
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
        }
        3 => {
            for _ in 0..4 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    None,
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
            if read_u32(body, 0x100).unwrap_or(0) != 0 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    Some(0),
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
        }
        4 => {
            if read_u32(body, 0xe0).unwrap_or(0) != 0 {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    Some(0),
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
        }
        5 => {
            let nested_count = read_u32(body, 0xec).unwrap_or(0) as usize;
            for _ in 0..nested_count {
                take_child(
                    data,
                    &mut cursor,
                    block_end,
                    None,
                    screen_width,
                    screen_height,
                    resource_count,
                    &mut children,
                );
            }
        }
        _ => {}
    }

    UiObject {
        index,
        serialized_type,
        block_offset,
        metadata: body.to_vec(),
        bounds: elements.first().copied(),
        elements,
        name,
        text,
        text_color,
        text_align,
        string_id,
        resource_indices,
        grid,
        children,
        raw: data
            .get(block_offset..block_end)
            .unwrap_or_default()
            .to_vec(),
    }
}

pub fn scan_png(data: &[u8], resources: &mut Vec<EmbeddedResource>) {
    let signature = b"\x89PNG\r\n\x1a\n";
    let mut cursor = 0;
    while cursor + signature.len() <= data.len() {
        let Some(relative) = data[cursor..]
            .windows(signature.len())
            .position(|w| w == signature)
        else {
            break;
        };
        let offset = cursor + relative;
        if let Some(end) = png_end(data, offset) {
            resources.push(EmbeddedResource {
                offset,
                end,
                kind: ResourceKind::Png,
                bytes: data[offset..end].to_vec(),
                width: None,
                height: None,
                format: None,
                decode_method: None,
            });
            cursor = end;
        } else {
            cursor = offset + signature.len();
        }
    }
}

pub fn png_end(data: &[u8], offset: usize) -> Option<usize> {
    let mut cursor = offset + 8;
    while cursor.checked_add(12)? <= data.len() {
        let length = u32::from_be_bytes(data.get(cursor..cursor + 4)?.try_into().ok()?) as usize;
        let end = cursor.checked_add(12)?.checked_add(length)?;
        if end > data.len() {
            return None;
        }
        if &data[cursor + 4..cursor + 8] == b"IEND" {
            return Some(end);
        }
        cursor = end;
    }
    None
}

pub fn scan_bmp(data: &[u8], resources: &mut Vec<EmbeddedResource>) {
    for offset in data
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == b"BM").then_some(offset))
    {
        let Some(size_bytes) = data.get(offset + 2..offset + 6) else {
            continue;
        };
        let size =
            u32::from_le_bytes(size_bytes.try_into().expect("slice length checked")) as usize;
        let Some(end) = offset.checked_add(size) else {
            continue;
        };
        if size >= 14 && end <= data.len() {
            resources.push(EmbeddedResource {
                offset,
                end,
                kind: ResourceKind::Bmp,
                bytes: data[offset..end].to_vec(),
                width: None,
                height: None,
                format: None,
                decode_method: None,
            });
        }
    }
}

pub fn read_u32(data: &[u8], offset: usize) -> Result<u32, ParseError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(ParseError::RangeOutOfBounds(offset))?;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("slice length checked"),
    ))
}

pub fn utf16_text(data: &[u8]) -> String {
    let units = data
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

fn read_rect(body: &[u8], offset: usize) -> Option<UiRect> {
    let bytes = body.get(offset..offset + 16)?;
    Some(UiRect {
        x: u32::from_le_bytes(bytes[0..4].try_into().ok()?),
        y: u32::from_le_bytes(bytes[4..8].try_into().ok()?),
        width: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
        height: u32::from_le_bytes(bytes[12..16].try_into().ok()?),
    })
}
