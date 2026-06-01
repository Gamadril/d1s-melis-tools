use binrw::binrw;

#[binrw]
#[br(little)]
#[derive(Debug, Clone)]
pub struct Toc1MainInfo {
    pub name: [u8; 16],     // Name of the image (usually "sunxi-package")
    pub magic: u32,         // Magic number (0x89119800)
    pub checksum: u32,      // Checksum
    pub serial: u32,        // Serial number/version
    pub status: u32,        // Status flags
    pub num_items: u32,     // Number of items in the container
    pub length: u32,        // Total length of the TOC1 image
    pub major_version: u32, // Major version
    pub minor_version: u32, // Minor version
    pub reserved: [u32; 3], // Reserved
    pub end: [u8; 4],       // End marker (e.g. "MIE;")
}

#[binrw]
#[br(little)]
#[derive(Debug, Clone)]
pub struct Toc1ItemInfo {
    pub name: [u8; 64],      // Name of the item (e.g. "melis-lzma", "melis-config")
    pub offset: u32,         // Offset of the item within the image (in bytes)
    pub length: u32,         // Length of the item
    pub encryption: u32,     // Encryption type
    pub type_val: u32,       // Item type
    pub load_addr: u32,      // Load address
    pub index: u32,          // Item index
    pub reserved: [u32; 69], // Reserved space
    pub end: [u8; 4],        // End marker (e.g. "IIE;")
}
