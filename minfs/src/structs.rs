use std::io::Cursor;
use std::vec::Vec;

use binrw::binrw;
use binrw::BinWrite;

const MINFS_MAGIC_LEN: u8 = 6;
pub const MINFS_HEADER_LEN: u32 = 0x0200;
pub const MINFS_MAGIC: &str = "MINFS\0";
pub const MINFS_DEFAULT_SECTION_NAME: &str = "MELIS_MOD";
pub const MINFS_MODULE_MAGIC: &str = "MAGIC";
pub const MINFS_SECTION_COMRPESS_MIN: u32 = 1024;
pub const DOTTEXT_OFFSET: usize = MINFS_DEFAULT_SECTION_NAME.len() + 1;
pub const DOTRODATA_OFFSET: usize = DOTTEXT_OFFSET + ".text".len() + 1;
pub const DOTDATA_OFFSET: usize = DOTRODATA_OFFSET + ".rodata".len() + 1;
pub const DOTBSS_OFFSET: usize = DOTDATA_OFFSET + ".data".len() + 1;
pub const DOTSHSTRTAB_OFFSET: usize = DOTBSS_OFFSET + ".bss".len() + 1;
pub const MAGIC_OFFSET: usize = DOTSHSTRTAB_OFFSET + ".shstrtab".len() + 1;
pub const MINFS_VERSION: u16 = 0x100;
pub const MINFS_NAME_ALIGN: u8 = 4;
pub const MINFS_DATA_ALIGN: u32 = 4;
pub const MINFS_DENTRY_ALIGN: u8 = 4;
pub const MINFS_SECTOR_LEN: u32 = 512;

pub const MINFS_ATTR_DIR: u16 = 1;
pub const MINFS_ATTR_MODULE: u16 = 2;
pub const MINFS_ATTR_COMPRESS: u16 = 4;

//attributes of minfs elf section header
pub const MINFS_SECTION_ATTR_MAGIC: u32 = 1;
pub const MINFS_SECTION_ATTR_COMPRESS: u32 = 2;

pub const EI_MAG0: u8 = 0x7F;
pub const EI_MAG1: u8 = b'E';
pub const EI_MAG2: u8 = b'L';
pub const EI_MAG3: u8 = b'F';
pub const ELFCLASS32: u8 = 1;
pub const ELFDATA2LSB: u8 = 1;
pub const EV_CURRENT: u8 = 1;
pub const ELFOSABI_NONE: u8 = 0;
pub const ET_EXEC: u16 = 2;
pub const EM_RISCV: u16 = 243;
pub const PT_LOAD: u32 = 1;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

// TODO saved in config file, read from there
pub const COMPRESS_EXT: [&str; 4] = ["axf", "drv", "mod", "plg"];

pub fn minfs_align(value: u32, alignment: u32) -> u32 {
    let mut result = value;
    let mod_rest = value % alignment;

    if mod_rest > 0 {
        result = result + (alignment - mod_rest);
    }

    return result;
}

// header of mini file system,
// byte size : 512.
#[binrw]
#[brw(little)]
#[derive(Debug)]
pub struct MinfsHeader {
    #[br(count = MINFS_MAGIC_LEN)]
    pub magic: Vec<u8>, // magic of mini file system
    pub version: u16,         // version number of mini file system
    pub root_dir_offset: u32, // byte offset of root directory
    pub root_dir_size: u32,   // byte size of root directory
    pub dentry_num: u32,      // the  number of directory entries
    pub dentry_len: u32,      // byte length of directory entries
    pub fdata_len: u32,       // byte size of file data area
    pub size: u32,            // byte size of mini file system image
    #[br(count = 480)] // reserved area
    pub reserved: Vec<u8>,
}

// directory entry of mini file system,
// name and extent is the meta stream of dentry,
// the layout of name and extent : |name ||padding||extent|,
// the padding mainly for name aligned by 4byte.
#[binrw]
#[brw(little)]
#[derive(Debug)]
pub struct MinfsDirEntry {
    pub offset: u32,      // offset of directory entry data
    pub size: u32,        // size of directory entry data
    pub unpack_size: u32, // size of uncompressed directory entry data
    pub record_len: u16,  // size of this dentry
    pub attribute: u16,   // attribute bits
    #[bw(calc = name.len() as u16)]
    name_len: u16, // size of dentry name
    #[bw(calc = (extent.len() * std::mem::size_of::<MinfsSectionHeader>()) as u16)]
    extent_len: u16, // offset of dentry extent data
    #[br(count = name_len)]
    #[brw(align_after = MINFS_NAME_ALIGN)]
    pub name: Vec<u8>, // meta data of dentry, name
    #[br(count = extent_len / std::mem::size_of::<MinfsSectionHeader>() as u16)]
    pub extent: Vec<MinfsSectionHeader>, //meta data of dentry, extent
}

#[allow(dead_code)]
impl MinfsDirEntry {
    pub fn size(&self) -> u16 {
        let mut buffer = Cursor::new(vec![0; 512]);
        self.write(&mut buffer).unwrap();
        return buffer.position() as u16;
    }
}

// extent entry of elf file,
// info extracted from elf section
#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinfsSectionHeader {
    pub offset: u32,               // offset of this section
    pub record_size: u32,          // size of this section in image
    pub record_unpack_size: u32,   // size of uncompressed in image
    pub size: u32,                 // size of this section data
    pub virtual_address: u32,      // virtual address of this section
    pub section_type: SectionType, // type of this section
    pub flags: u32,                // flags of this section
    pub attribute: u32,            // attribute bits of this section
}

#[binrw]
#[brw(little)]
pub struct Elf32Ident {
    pub ei_mag0: u8,
    pub ei_mag1: u8,
    pub ei_mag2: u8,
    pub ei_mag3: u8,
    pub ei_class: u8,
    pub ei_data: u8,
    pub ei_version: u8,
    pub ei_osabi: u8,
    pub ei_abiversion: u8,
    pub ei_pad0: u8,
    pub ei_pad1: u8,
    pub ei_pad2: u8,
    pub ei_pad3: u8,
    pub ei_pad4: u8,
    pub ei_pad5: u8,
    pub ei_nident: u8,
}

#[binrw]
#[brw(little)]
pub struct Elf32Ehdr {
    pub e_ident: Elf32Ident, // magic number and other info
    pub e_type: u16,         // object file type
    pub e_machine: u16,      // architecture
    pub e_version: u32,      // file version
    pub e_entry: u32,        // virtual address of entry point
    pub e_phoff: u32,        // program header table's file offset
    pub e_shoff: u32,        // section header table's file offset
    pub e_flags: u32,        // processor-specific flags
    pub e_ehsize: u16,       // ELF header's size
    pub e_phentsize: u16,    // size of one entry in the file's program header table
    pub e_phnum: u16,        // number of entries in the program header table
    pub e_shentsize: u16,    // sections header's size
    pub e_shnum: u16,        // number of entries in the section header table
    pub e_shstrndx: u16,     // section header string table index
}

// section header
#[binrw]
#[brw(little)]
#[derive(Debug)]
pub struct Elf32Shdr {
    pub sh_name: u32, // name of the section (index into section header string table)
    pub sh_type: SectionType, // section type
    pub sh_flags: u32, // section flags
    pub sh_addr: u32, // section virtual address att runtime
    pub sh_offset: u32, // section file offset
    pub sh_size: u32, // section size
    pub sh_link: u32, // section header table index link
    pub sh_info: u32, // extra info
    pub sh_addralign: u32, // section address alignment
    pub sh_entsize: u32, // entry size if table of fixed-sized entries exist
}

// Program segment header
#[binrw]
#[brw(little)]
#[derive(Debug)]
pub struct Elf32Phdr {
    pub p_type: u32,   // segment type
    pub p_offset: u32, // segment file offset
    pub p_vaddr: u32,  // segment virtual address
    pub p_paddr: u32,  // segment physical address
    pub p_filesz: u32, // segment size in file
    pub p_memsz: u32,  // segment size in memory
    pub p_flags: u32,  // segment flags
    pub p_align: u32,  // segment alignment in memory
}

#[binrw]
#[brw(little, repr(u32))]
#[allow(non_camel_case_types)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SectionType {
    SHT_NULL = 0,
    SHT_PROGBITS = 1,
    SHT_SYMTAB = 2,
    SHT_STRTAB = 3,
    SHT_RELA = 4,
    SHT_HASH = 5,
    SHT_DYNAMIC = 6,
    SHT_NOTE = 7,
    SHT_NOBITS = 8,
    SHT_REL = 9,
    SHT_SHLIB = 10,
    SHT_DYNSYM = 11,
    SHT_INIT_ARRAY = 14,
    SHT_FINI_ARRAY = 15,
    SHT_PREINIT_ARRAY = 16,
    SHT_GROUP = 17,
    SHT_SYMTAB_SHNDX = 18,
    SHT_NUM = 19,
}

#[binrw]
#[brw(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LzmaProbs {
    pub props: u8,
    pub dict_size: u32,
}
