use thiserror::Error;

pub const HEADER_SIZE: usize = 0x20;
pub const MAX_SECTION_METADATA: usize = 0x1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataHeader {
    pub magic: [u8; 8],
    pub version: [u8; 8],
    pub header_size: u32,
    pub main_size: u32,
    pub reserved0: u32,
    pub reserved1: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub offset: usize,
    pub end: usize,
    pub declared_size: u32,
    pub metadata_size: u32,
    pub metadata: Vec<u8>,
    pub metadata_words: Vec<u32>,
    pub descriptor_offset: usize,
    pub descriptor_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    pub index: usize,
    pub offset: usize,
    pub first: u32,
    pub second: u32,
    pub kind: Option<u32>,
    pub payload_offset: Option<usize>,
    pub payload_end: Option<usize>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFile {
    pub bytes: Vec<u8>,
    pub header: DataHeader,
    pub section: Section,
    pub descriptors: Vec<Descriptor>,
    pub resources: Vec<EmbeddedResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiGrid {
    pub columns: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
}

/// Type-0 text control: horizontal align in metadata +0x1c0 bits 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiObject {
    pub index: usize,
    pub serialized_type: u32,
    pub block_offset: usize,
    pub metadata: Vec<u8>,
    pub bounds: Option<UiRect>,
    pub elements: Vec<UiRect>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub text_color: Option<(u8, u8, u8)>,
    pub text_align: Option<TextAlign>,
    pub string_id: Option<u32>,
    pub resource_indices: Vec<u32>,
    pub grid: Option<UiGrid>,
    pub children: Vec<UiObject>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiScene {
    pub width: u32,
    pub height: u32,
    pub objects: Vec<UiObject>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeMethod {
    Raw,
    RowRunLength,
    PaletteRunLength,
    Compressed,
    Unknown(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedResource {
    pub offset: usize,
    pub end: usize,
    pub kind: ResourceKind,
    pub bytes: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<u32>,
    pub decode_method: Option<DecodeMethod>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Png,
    Bmp,
    Utf16,
    Surface,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ParseError {
    #[error("file is smaller than the 0x20-byte runtime header")]
    ShortHeader,
    #[error("invalid DATA magic")]
    InvalidMagic,
    #[error("header size {0:#x} is smaller than the runtime header")]
    InvalidHeaderSize(u32),
    #[error("header range extends past the file")]
    HeaderOutOfBounds,
    #[error("section range extends past the file")]
    SectionOutOfBounds,
    #[error("section metadata is larger than the supported runtime maximum")]
    MetadataTooLarge,
    #[error("descriptor count overflows")]
    DescriptorCountOverflow,
    #[error("descriptor table extends past the main section")]
    DescriptorTableOutOfBounds,
    #[error("decode method {0:?} requires a type-specific codec")]
    UnsupportedDecodeMethod(DecodeMethod),
    #[error("integer range at {0:#x} extends past the file")]
    RangeOutOfBounds(usize),
    #[error("resource decompression failed: {0}")]
    DecompressionFailed(String),
}
