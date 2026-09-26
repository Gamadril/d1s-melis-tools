use melis_data::UiObject;

/// Byte offset (from the object's rect_offset) into a type-0 text control's
/// 200-byte UTF-16 text field. Mirrors melis_data::decode::decode_ui_block.
const TEXT_FIELD_OFFSET: usize = 0xcc;
const TEXT_FIELD_CAPACITY: usize = 0x194 - 0xcc; // 200 bytes / 100 UTF-16 units

/// Byte offset of the resource-index list within a UI block's metadata body.
const RESOURCE_LIST_OFFSET: usize = 0xdc;

pub const RESOURCE_SLOT_UNUSED: u32 = u32::MAX;

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Absolute byte offset of the UiRect (x,y,width,height) for this object,
/// same formula data_renderer/melis-data use to decode it.
pub fn rect_byte_offset(object: &UiObject) -> usize {
    let rect_offset = if object.serialized_type == 0 {
        0x194
    } else {
        0xcc
    };
    object.block_offset + 8 + rect_offset
}

/// Patch an object's on-screen rectangle in place. The 16-byte slot already
/// exists in the file (decoding only succeeds if it does), so this never
/// changes the file's size or layout.
pub fn set_bounds(bytes: &mut [u8], object: &UiObject, x: u32, y: u32, width: u32, height: u32) {
    let offset = rect_byte_offset(object);
    write_u32(bytes, offset, x);
    write_u32(bytes, offset + 4, y);
    write_u32(bytes, offset + 8, width);
    write_u32(bytes, offset + 12, height);
}

/// True if this object has a type-0 text field we know how to locate and
/// safely overwrite (i.e. its metadata body reaches at least past the rect
/// that follows the text field).
pub fn has_editable_text(object: &UiObject) -> bool {
    object.serialized_type == 0 && object.bounds.is_some()
}

pub fn text_capacity_chars() -> usize {
    // one UTF-16 unit reserved for a null terminator
    TEXT_FIELD_CAPACITY / 2 - 1
}

/// Overwrite a type-0 control's UTF-16 text in place, truncating to fit the
/// existing 200-byte field and zero-padding the remainder (null terminated,
/// matching how the firmware stores it).
pub fn set_text(bytes: &mut [u8], object: &UiObject, text: &str) {
    let offset = object.block_offset + 8 + TEXT_FIELD_OFFSET;
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.truncate(text_capacity_chars());
    let mut buf = [0u8; TEXT_FIELD_CAPACITY];
    for (index, unit) in units.iter().enumerate() {
        buf[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    bytes[offset..offset + TEXT_FIELD_CAPACITY].copy_from_slice(&buf);
}

/// How many resource-index slots this object type exposes, and whether they
/// are safe to edit in place (type 4 nests further objects instead of a
/// simple index list and is intentionally left read-only here).
pub fn resource_slot_count(object: &UiObject) -> usize {
    match object.serialized_type {
        1 | 2 => 4,
        5 => 1,
        _ => 0,
    }
}

pub fn resource_slot_value(bytes: &[u8], object: &UiObject, slot: usize) -> u32 {
    let offset = object.block_offset + 8 + RESOURCE_LIST_OFFSET + slot * 4;
    read_u32(bytes, offset)
}

/// Point one of an object's resource slots at a different (or newly added)
/// resource index. Pass `RESOURCE_SLOT_UNUSED` to clear a slot.
pub fn set_resource_slot(bytes: &mut [u8], object: &UiObject, slot: usize, value: u32) {
    let offset = object.block_offset + 8 + RESOURCE_LIST_OFFSET + slot * 4;
    write_u32(bytes, offset, value);
}
