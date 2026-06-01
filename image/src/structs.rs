use binrw::{BinRead, BinWrite};

const IMAGE_MAGIC_LEN: u32 = 8;
pub const IMAGE_MAGIC: &str = "IMAGEWTY";
pub const IMAGE_HEADER_VERSION: u32 = 0x0300;

pub const IMAGE_ITEM_VERSION: u32 = 0x0100;
pub const MAINTYPE_LEN: u32 = 8;
pub const SUBTYPE_LEN: u32 = 16;
pub const FILE_PATH_LEN: u32 = 256;

#[derive(BinRead, BinWrite, Debug, Clone)]
#[br(little)]
#[bw(little)]
pub struct ImageHeader {
    #[br(count = IMAGE_MAGIC_LEN)]
    pub magic: Vec<u8>,
    pub version: u32, // header version
    pub size: u32,    // header size
    pub attributes: u32,
    pub image_version: u32,
    pub image_size: u64,
    pub alignment: u32,
    pub pid: u32,
    pub vid: u32,
    pub hardware_id: u32,
    pub firmware_id: u32,
    pub item_attr: u32,
    pub item_size: u32,
    pub item_count: u32,
    pub item_offset: u32,
    pub image_attr: u32,
    pub append_size: u32,
    pub append_offset: u64,
    #[br(count = 12)]
    pub reserved: Vec<u8>,
}

#[derive(BinRead, BinWrite, Debug, Clone)]
#[br(little)]
#[bw(little)]
pub struct ImageItem {
    pub version: u32,
    pub size: u32,
    #[br(count = MAINTYPE_LEN)]
    pub main_type: Vec<u8>,
    #[br(count = SUBTYPE_LEN)]
    pub sub_type: Vec<u8>,
    pub attributes: u32,
    #[br(count = FILE_PATH_LEN)]
    pub name: Vec<u8>,
    pub data_length: u64,
    pub file_length: u64,
    pub offset: u64,
    #[br(count = 64)]
    pub encrypt_id: Vec<u8>,
    pub checksum: u32,
    #[br(count = 640)]
    pub reserved: Vec<u8>,
}

pub fn pad_bytes(data: &[u8], len: usize) -> Vec<u8> {
    let mut v = data.to_vec();
    v.resize(len, 0);
    v
}
