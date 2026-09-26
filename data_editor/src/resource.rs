use melis_data::DataFile;

/// Re-derives where the trailing `[count:u32][(offset:u32,size:u32); count]`
/// resource table lives, using the same candidate offsets and validity
/// checks as melis_data's internal `parse_resource_table` (kept in sync by
/// hand since that helper isn't exported).
fn locate_resource_table(bytes: &[u8], file: &DataFile) -> Option<(usize, u32)> {
    let section_end = file.section.end;
    let main_size = file.header.main_size as usize;
    let candidates = [section_end, main_size, 0x20 + main_size];
    for &candidate in &candidates {
        let table = bytes.get(candidate..)?;
        if table.len() < 4 {
            continue;
        }
        let count = u32::from_le_bytes(table[..4].try_into().ok()?);
        let count_usize = count as usize;
        if count_usize == 0 || count_usize > 512 || 4 + count_usize * 8 > table.len() {
            continue;
        }
        let mut ok = true;
        for index in 0..count_usize {
            let at = 4 + index * 8;
            let Some(offset_bytes) = table.get(at..at + 4) else {
                ok = false;
                break;
            };
            let Some(size_bytes) = table.get(at + 4..at + 8) else {
                ok = false;
                break;
            };
            let offset = u32::from_le_bytes(offset_bytes.try_into().ok()?) as usize;
            let size = u32::from_le_bytes(size_bytes.try_into().ok()?) as usize;
            let end = offset.saturating_add(24).saturating_add(size);
            if offset >= bytes.len() || end > bytes.len() {
                ok = false;
                break;
            }
        }
        if ok {
            return Some((candidate, count));
        }
    }
    None
}

/// Appends a new uncompressed (Raw, 32bpp) surface resource. The table
/// actually sits *before* the resource data it describes
/// (`[main][count][entries...][resource0][resource1]...`), confirmed against
/// a real Main.data: its table's 20 entries end exactly where resource[0]
/// starts. So adding a resource means:
///   1. growing the table by one 8-byte entry (shifts every existing
///      resource's absolute offset forward by those 8 bytes — their sizes
///      and relative order don't change, only where they now sit),
///   2. appending the new resource's bytes after the old last resource.
///
/// Returns the new resource's index (its position in `file.resources`,
/// i.e. the value to place in a UI object's resource slot).
pub fn insert_surface_resource(
    bytes: &mut Vec<u8>,
    file: &DataFile,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<usize, String> {
    let (table_offset, count) = locate_resource_table(bytes, file)
        .ok_or("could not locate this file's resource table (unsupported/unrecognized layout)")?;
    if rgba.len() < width as usize * height as usize * 4 {
        return Err("decoded image is smaller than width*height*4 bytes".into());
    }

    const BPP: usize = 4; // format 1 = 32bpp, matches surface_bytes_per_pixel(1)
    const FORMAT_32BPP: u32 = 1;
    const DECODE_RAW: u32 = 0;

    let stride = (width as usize * BPP + 3) & !3;
    let mut payload = vec![0u8; stride * height as usize];
    for row in 0..height as usize {
        for col in 0..width as usize {
            let src = &rgba[(row * width as usize + col) * 4..][..4];
            let dst = row * stride + col * 4;
            // The renderer's 32bpp surface decoder (data_renderer::surface)
            // reads pixels as B,G,R,A — swap channels to match.
            payload[dst] = src[2];
            payload[dst + 1] = src[1];
            payload[dst + 2] = src[0];
            payload[dst + 3] = src[3];
        }
    }

    let mut resource_bytes = Vec::with_capacity(24 + payload.len());
    resource_bytes.extend_from_slice(&0u32.to_le_bytes()); // reserved / payload-offset override
    resource_bytes.extend_from_slice(&width.to_le_bytes());
    resource_bytes.extend_from_slice(&height.to_le_bytes());
    resource_bytes.extend_from_slice(&FORMAT_32BPP.to_le_bytes());
    resource_bytes.extend_from_slice(&DECODE_RAW.to_le_bytes());
    resource_bytes.extend_from_slice(&0u32.to_le_bytes()); // reserved
    resource_bytes.extend_from_slice(&payload);

    let old_table_len = 4 + count as usize * 8;
    let resources_start = table_offset + old_table_len;
    let old_table = bytes[table_offset..resources_start].to_vec();

    const GROWTH: u32 = 8; // one new (offset, size) entry added to the table
    let new_entry_offset = bytes.len() as u32 + GROWTH; // appended after everything, post-shift
    let new_entry_size = payload.len() as u32;

    let mut new_table = Vec::with_capacity(old_table_len + GROWTH as usize);
    new_table.extend_from_slice(&(count + 1).to_le_bytes());
    for index in 0..count as usize {
        let at = 4 + index * 8;
        let old_offset = u32::from_le_bytes(old_table[at..at + 4].try_into().unwrap());
        let size = u32::from_le_bytes(old_table[at + 4..at + 8].try_into().unwrap());
        new_table.extend_from_slice(&(old_offset + GROWTH).to_le_bytes());
        new_table.extend_from_slice(&size.to_le_bytes());
    }
    new_table.extend_from_slice(&new_entry_offset.to_le_bytes());
    new_table.extend_from_slice(&new_entry_size.to_le_bytes());

    let mut out = Vec::with_capacity(bytes.len() + resource_bytes.len() + GROWTH as usize);
    out.extend_from_slice(&bytes[..table_offset]);
    out.extend_from_slice(&new_table);
    out.extend_from_slice(&bytes[resources_start..]); // old resource data, unchanged content
    out.extend_from_slice(&resource_bytes); // new resource appended at the very end
    *bytes = out;

    Ok(count as usize)
}
