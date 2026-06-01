use binrw::BinRead;

#[derive(PartialEq, Eq, Debug, BinRead)]
pub enum BootloaderType {
    #[br(magic = b"eGON.BT0")]
    EGon0,
    #[br(magic = b"eGON.BT1")]
    EGon1,
}

#[allow(dead_code)]
#[derive(BinRead, Debug)]
#[br(little)]
pub struct BootloaderHead {
    jump_instruction: u32,
    magic: BootloaderType,
    check_sum: u32,
    pub length: u32,
    pub_head_size: u32,
    pub_head_vsn: u32,
    ret_addr: u32,
    run_addr: u32,
    boot_cpu: u32,
    #[br(count = 8)]
    platform: Vec<u8>, // platform information
}
