use decode::*;
use structs::*;

pub mod decode;
pub mod language;
pub mod structs;

pub use decode::{decode_resource, surface_bytes_per_pixel, utf16_text};
pub use language::{load_language_dir, parse_language_table};
pub use structs::*;

const DATA_MAGIC: [u8; 8] = *b"D\0A\0T\0A\0";

impl Section {
    pub fn screen_size(&self) -> Option<(u32, u32)> {
        Some((*self.metadata_words.get(2)?, *self.metadata_words.get(3)?))
    }

    pub fn descriptor_count(&self) -> usize {
        self.metadata_words.get(4).copied().unwrap_or(0) as usize
    }
}

impl Descriptor {
    pub fn fields(&self) -> [u32; 2] {
        [self.first, self.second]
    }

    pub fn decode_method(&self) -> Option<DecodeMethod> {
        self.fields()
            .into_iter()
            .find(|value| *value <= 3)
            .map(DecodeMethod::from)
    }
}

impl DataFile {
    pub fn scene(&self) -> UiScene {
        let (width, height) = self.section.screen_size().unwrap_or((1024, 600));
        let objects = self
            .descriptors
            .iter()
            .filter(|descriptor| descriptor.kind == Some(5))
            .enumerate()
            .map(|(index, descriptor)| {
                let offset = descriptor.second as usize;
                decode_ui_block(
                    index,
                    5,
                    offset,
                    &self.bytes,
                    width,
                    height,
                    self.resources.len(),
                )
            })
            .collect();
        UiScene {
            width,
            height,
            objects,
        }
    }
}

impl From<u32> for DecodeMethod {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::Raw,
            1 => Self::RowRunLength,
            2 => Self::PaletteRunLength,
            3 => Self::Compressed,
            other => Self::Unknown(other),
        }
    }
}

fn range(data: &[u8], start: usize, len: usize) -> Result<&[u8], ParseError> {
    let end = start
        .checked_add(len)
        .ok_or(ParseError::RangeOutOfBounds(start))?;
    data.get(start..end)
        .ok_or(ParseError::RangeOutOfBounds(start))
}

pub fn parse(data: &[u8]) -> Result<DataFile, ParseError> {
    if data.len() < HEADER_SIZE {
        return Err(ParseError::ShortHeader);
    }
    let header = DataHeader {
        magic: data[0..8].try_into().expect("slice length checked"),
        version: data[8..16].try_into().expect("slice length checked"),
        header_size: read_u32(data, 0x10)?,
        main_size: read_u32(data, 0x14)?,
        reserved0: read_u32(data, 0x18)?,
        reserved1: read_u32(data, 0x1c)?,
    };
    if header.magic != DATA_MAGIC {
        return Err(ParseError::InvalidMagic);
    }
    if header.header_size < HEADER_SIZE as u32 {
        return Err(ParseError::InvalidHeaderSize(header.header_size));
    }
    let section_offset = header.header_size as usize;
    let section_size = read_u32(data, section_offset)?;
    let metadata_size = read_u32(data, section_offset + 4)?;
    let metadata_len = metadata_size as usize;
    if metadata_len > MAX_SECTION_METADATA {
        return Err(ParseError::MetadataTooLarge);
    }
    let metadata_offset = section_offset + 8;
    let metadata = range(data, metadata_offset, metadata_len)?.to_vec();
    let metadata_words = metadata
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("chunk length checked")))
        .collect::<Vec<_>>();
    let section_end = section_offset
        .checked_add(section_size as usize)
        .ok_or(ParseError::SectionOutOfBounds)?;
    if section_end > data.len() {
        return Err(ParseError::SectionOutOfBounds);
    }
    let descriptor_offset = metadata_offset
        .checked_add(metadata_len)
        .ok_or(ParseError::DescriptorTableOutOfBounds)?;
    let descriptor_count = metadata_words.get(4).copied().unwrap_or(0) as usize;
    let descriptor_bytes = descriptor_count
        .checked_mul(8)
        .ok_or(ParseError::DescriptorCountOverflow)?;
    let descriptor_end = descriptor_offset
        .checked_add(descriptor_bytes)
        .ok_or(ParseError::DescriptorTableOutOfBounds)?;
    if descriptor_end > section_end {
        return Err(ParseError::DescriptorTableOutOfBounds);
    }

    let section = Section {
        offset: section_offset,
        end: section_end,
        declared_size: section_size,
        metadata_size,
        metadata,
        metadata_words,
        descriptor_offset,
        descriptor_end,
    };
    let mut descriptors = Vec::with_capacity(descriptor_count);
    for index in 0..descriptor_count {
        let offset = descriptor_offset + index * 8;
        let first = read_u32(data, offset)?;
        let second = read_u32(data, offset + 4)?;
        let kind = [first, second]
            .into_iter()
            .find(|value| *value == 5 || *value == 6);
        let payload_offset = [first, second]
            .into_iter()
            .map(|value| value as usize)
            .find(|value| *value >= descriptor_end && *value < data.len());
        descriptors.push(Descriptor {
            index,
            offset,
            first,
            second,
            kind,
            payload_offset,
            payload_end: None,
            payload: Vec::new(),
        });
    }
    let payload_offsets = descriptors
        .iter()
        .filter_map(|descriptor| descriptor.payload_offset);
    let mut boundaries = payload_offsets.collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();
    for descriptor in &mut descriptors {
        let Some(start) = descriptor.payload_offset else {
            continue;
        };
        let end = boundaries
            .iter()
            .copied()
            .find(|candidate| *candidate > start)
            .unwrap_or(section_end);
        descriptor.payload_end = Some(end);
        descriptor.payload = data[start..end].to_vec();
    }

    let size = header.main_size as usize;

    Ok(DataFile {
        bytes: data.to_vec(),
        header,
        section: section.clone(),
        descriptors,
        resources: parse_resource_table(data, section.end, size)
            .unwrap_or_else(|| detect_resources(data)),
    })
}

pub fn detect_resources(data: &[u8]) -> Vec<EmbeddedResource> {
    let mut resources = Vec::new();
    scan_png(data, &mut resources);
    scan_bmp(data, &mut resources);
    resources.sort_by_key(|resource| resource.offset);
    resources
}

fn parse_resource_table(
    data: &[u8],
    section_end: usize,
    main_size: usize,
) -> Option<Vec<EmbeddedResource>> {
    let candidate_offsets = [section_end, main_size, 0x20 + main_size];
    for &candidate in &candidate_offsets {
        let Some(table) = data.get(candidate..) else {
            continue;
        };
        let Some(count_val) = table
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
        else {
            continue;
        };
        let count = count_val as usize;
        if count == 0 || count > 512 || 4 + count.saturating_mul(8) > table.len() {
            continue;
        }
        let mut resources = Vec::with_capacity(count);
        let mut valid = true;
        for index in 0..count {
            let at = 4 + index * 8;
            let Some(offset_bytes) = table.get(at..at + 4).and_then(|b| b.try_into().ok()) else {
                valid = false;
                break;
            };
            let Some(size_bytes) = table.get(at + 4..at + 8).and_then(|b| b.try_into().ok()) else {
                valid = false;
                break;
            };
            let offset = u32::from_le_bytes(offset_bytes) as usize;
            let size = u32::from_le_bytes(size_bytes) as usize;
            let end = offset.saturating_add(24).saturating_add(size);
            if offset >= data.len() || end > data.len() {
                valid = false;
                break;
            }
            let bytes = data.get(offset..end).unwrap_or_default().to_vec();
            let (width, height, format, decode_method) = if bytes.len() >= 0x18 {
                (
                    Some(u32::from_le_bytes(bytes[4..8].try_into().ok()?)),
                    Some(u32::from_le_bytes(bytes[8..12].try_into().ok()?)),
                    Some(u32::from_le_bytes(bytes[12..16].try_into().ok()?)),
                    Some(DecodeMethod::from(u32::from_le_bytes(
                        bytes[16..20].try_into().ok()?,
                    ))),
                )
            } else {
                (None, None, None, None)
            };
            resources.push(EmbeddedResource {
                offset,
                end,
                kind: ResourceKind::Surface,
                bytes,
                width,
                height,
                format,
                decode_method,
            });
        }
        if valid && !resources.is_empty() {
            return Some(resources);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_exact_descriptor_table() {
        let mut data = vec![0; 0x80];
        data[0..8].copy_from_slice(&DATA_MAGIC);
        data[8..16].copy_from_slice(b"V\0\x31\0.\0\x30\0");
        data[0x10..0x14].copy_from_slice(&0x20u32.to_le_bytes());
        data[0x20..0x24].copy_from_slice(&0x60u32.to_le_bytes());
        data[0x24..0x28].copy_from_slice(&20u32.to_le_bytes());
        data[0x38..0x3c].copy_from_slice(&1u32.to_le_bytes());
        data[0x3c..0x40].copy_from_slice(&0x50u32.to_le_bytes());
        data[0x40..0x44].copy_from_slice(&5u32.to_le_bytes());
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.section.descriptor_count(), 1);
        assert_eq!(parsed.descriptors[0].kind, Some(5));
        assert_eq!(parsed.descriptors[0].payload_offset, Some(0x50));
    }

    #[test]
    fn rejects_truncated_section() {
        let mut data = vec![0; HEADER_SIZE];
        data[0..8].copy_from_slice(&DATA_MAGIC);
        data[0x10..0x14].copy_from_slice(&0x20u32.to_le_bytes());
        assert_eq!(parse(&data), Err(ParseError::RangeOutOfBounds(0x20)));
    }

    #[test]
    fn surface_format_3_is_24bit() {
        assert_eq!(surface_bytes_per_pixel(1), Some(4));
        assert_eq!(surface_bytes_per_pixel(2), Some(2));
        assert_eq!(surface_bytes_per_pixel(3), Some(3));
        assert_eq!(surface_bytes_per_pixel(0), None);
    }

    #[test]
    fn calendar_dump_matches_firmware_layout() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/Calendar.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let parsed = parse(&bytes).expect("calendar.data should parse");
        assert_eq!(parsed.resources.len(), 20);
        for resource in &parsed.resources {
            let decoded = decode_resource(resource).expect("surface decode");
            let width = resource.width.expect("width") as usize;
            let height = resource.height.expect("height") as usize;
            let bpp = surface_bytes_per_pixel(resource.format.expect("format")).expect("bpp");
            let stride = (width * bpp + 3) & !3;
            assert!(
                decoded.len() >= stride * height,
                "{}x{} fmt={:?} decoded={} need={}",
                width,
                height,
                resource.format,
                decoded.len(),
                stride * height
            );
        }
        let scene = parsed.scene();
        assert_eq!(scene.objects.len(), 1);
        let view = &scene.objects[0];
        assert_eq!(view.resource_indices, vec![0]);
        assert_eq!(view.children.len(), 20);
        let year = view
            .children
            .iter()
            .find(|child| child.name.as_deref() == Some("ViewShowStrSolarYear"))
            .expect("year label");
        assert_eq!(year.text.as_deref(), Some("2014"));
        assert_eq!(
            year.bounds,
            Some(UiRect {
                x: 97,
                y: 28,
                width: 150,
                height: 40
            })
        );
        let grid = view
            .children
            .iter()
            .find(|child| child.serialized_type == 3)
            .expect("day list");
        assert_eq!(
            grid.grid,
            Some(UiGrid {
                columns: 7,
                rows: 6,
                cell_width: 103,
                cell_height: 63
            })
        );
        assert!(!grid.children.is_empty());
    }

    #[test]
    fn main_buttons_use_nested_text_ctrl() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/Main.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let scene = parse(&bytes).expect("main.data should parse").scene();
        let phone = scene.objects[1]
            .children
            .iter()
            .find(|child| child.string_id == Some(519))
            .expect("phone link button");
        assert_eq!(phone.children.len(), 1);
        assert_eq!(phone.children[0].serialized_type, 0);
        assert_eq!(phone.children[0].text.as_deref(), Some("手机互联"));
        assert_eq!(
            phone.children[0].bounds,
            Some(UiRect {
                x: 0,
                y: 110,
                width: 160,
                height: 30
            })
        );
        assert_eq!(phone.children[0].text_align, Some(TextAlign::Center));
    }

    #[test]
    fn setup_debug_labels_use_left_and_right_align() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/SetupDebug.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let scene = parse(&bytes).expect("SetupDebug.data should parse").scene();
        let view = &scene.objects[0];
        let mem = view
            .children
            .iter()
            .find_map(|child| {
                child
                    .children
                    .iter()
                    .find(|nested| nested.text.as_deref() == Some("显示内存信息"))
            })
            .expect("memory label");
        assert_eq!(mem.text_align, Some(TextAlign::Left));
        let shot = view
            .children
            .iter()
            .find(|child| child.name.as_deref() == Some("ViewShowStrScreenShotSw"))
            .expect("screenshot value");
        assert_eq!(shot.text.as_deref(), Some("开"));
        assert_eq!(shot.text_align, Some(TextAlign::Right));
        let start = view
            .children
            .iter()
            .find_map(|child| {
                child
                    .children
                    .iter()
                    .find(|nested| nested.text.as_deref() == Some("开始"))
            })
            .expect("start button caption");
        assert_eq!(start.text_align, Some(TextAlign::Center));
    }

    #[test]
    fn main_app_keeps_offscreen_page_icons() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/MainApp.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let scene = parse(&bytes).expect("MainApp.data should parse").scene();
        let grid = scene
            .objects
            .iter()
            .find(|object| {
                object.bounds
                    == Some(UiRect {
                        x: 0,
                        y: 85,
                        width: 1024,
                        height: 330,
                    })
            })
            .expect("app grid view");
        let offscreen: Vec<_> = grid
            .children
            .iter()
            .filter_map(|child| child.bounds)
            .filter(|rect| rect.x > 1024)
            .collect();
        assert_eq!(
            offscreen,
            vec![
                UiRect {
                    x: 1084,
                    y: 0,
                    width: 150,
                    height: 160
                },
                UiRect {
                    x: 1274,
                    y: 0,
                    width: 150,
                    height: 160
                },
            ]
        );
    }

    #[test]
    fn system_bar_is_top_overlay() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/SystemBar.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let scene = parse(&bytes).expect("SystemBar.data should parse").scene();
        let bar = scene
            .objects
            .iter()
            .find(|object| object.name.as_deref() == Some("ViewShowWndSystemBar"))
            .expect("idle system bar view");
        let bounds = bar.bounds.expect("system bar rect");
        assert_eq!(bounds.x, 0);
        assert!(
            bounds.y < 70,
            "bar y={} should sit above Main y=70",
            bounds.y
        );
        assert_eq!(bounds.height, 64);
        assert!(
            scene
                .objects
                .iter()
                .any(|object| object.name.as_deref() == Some("ViewShowWndSystemBarPullDown")),
            "pulldown is a second view, not idle chrome"
        );
    }

    #[test]
    fn main_link_icon_surface_is_smaller_than_button() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/MainLink.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let parsed = parse(&bytes).expect("MainLink.data should parse");
        let scene = parsed.scene();
        let carplay = scene.objects[0]
            .children
            .iter()
            .find(|child| child.string_id == Some(520))
            .expect("CarPlay button");
        assert_eq!(
            carplay.bounds,
            Some(UiRect {
                x: 150,
                y: 155,
                width: 126,
                height: 160
            })
        );
        let idle = carplay.resource_indices[0] as usize;
        assert_eq!(parsed.resources[idle].width, Some(126));
        assert_eq!(parsed.resources[idle].height, Some(126));
    }

    #[test]
    fn btbook_panel_surface_is_narrower_than_view() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/BtBook.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let parsed = parse(&bytes).expect("BtBook.data should parse");
        let scene = parsed.scene();
        let panel = scene
            .objects
            .iter()
            .find(|object| {
                object.bounds
                    == Some(UiRect {
                        x: 487,
                        y: 67,
                        width: 535,
                        height: 432,
                    })
            })
            .expect("right-hand panel view");
        let idle = panel.resource_indices[0] as usize;
        assert_eq!(parsed.resources[idle].width, Some(531));
        assert_eq!(parsed.resources[idle].height, Some(432));
        let backspace = panel
            .children
            .iter()
            .find(|child| child.string_id == Some(0x400b))
            .expect("backspace");
        assert_eq!(backspace.resource_indices, vec![19, 20]);
    }

    #[test]
    fn car_computer_type4_binds_body_and_doors() {
        let path = "/home/dwi/projects/privat/F133/workspace/dump_hz_b500_1_7.bin.out/gpt.bin.out/2_ROOTFS.bin.out/apps/Data/CarComputer.data";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let parsed = parse(&bytes).expect("CarComputer.data should parse");
        let scene = parsed.scene();
        let view = &scene.objects[0];
        let body = view
            .children
            .iter()
            .find(|child| {
                child.serialized_type == 4
                    && child.bounds
                        == Some(UiRect {
                            x: 451,
                            y: 168,
                            width: 126,
                            height: 275,
                        })
            })
            .expect("car body");
        assert_eq!(body.resource_indices, vec![0]);
        assert_eq!(parsed.resources[0].width, Some(126));
        assert_eq!(parsed.resources[0].height, Some(275));
        let fl = view
            .children
            .iter()
            .find(|child| child.name.as_deref() == Some("ViewShowImageFLDoor"))
            .expect("FL door");
        assert_eq!(fl.resource_indices, vec![1]);
        let trunk = view
            .children
            .iter()
            .find(|child| child.name.as_deref() == Some("ViewShowImageTruck"))
            .expect("trunk");
        assert_eq!(trunk.resource_indices, vec![5]);
    }
}
