use std::vec::Vec;

use binrw::BinRead;

#[allow(dead_code)]
#[derive(BinRead, Debug)]
#[br(little)]
pub struct GPTHeader {
    #[br(count = 8)]
    signature: Vec<u8>,
    #[br(count = 4)]
    revision: Vec<u8>,
    header_size: u32,
    header_crc32: i32,
    reserved: i32,
    my_lba: u64,
    alternate_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    //type::GUID DiskGUID,
    #[br(count = 16)]
    disk_guid: Vec<u8>,
    partition_entry_lba: u64,
    pub number_of_partition_entries: u32,
    sizeof_partition_entry: u32,
    partition_entry_array_crc32: u32,
    #[br(count = 420)]
    reserved_empty: Vec<u8>,
}

#[allow(dead_code)]
#[derive(BinRead, Debug)]
#[br(little)]
pub struct EFIPartitionEntry {
    #[br(count = 16)]
    pub partition_type_guid: Vec<u8>,
    #[br(count = 16)]
    unique_partition_guid: Vec<u8>,
    pub starting_lba: u64,
    pub ending_lba: u64,
    attributes: u64,
    #[br(count = 36)]
    pub partition_name: Vec<u16>,
}
