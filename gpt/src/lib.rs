use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use binrw::io::BufReader;
use binrw::io::Read;
use binrw::io::Seek;
use binrw::io::Write;
use binrw::BinRead;
use structs::*;

pub mod structs;

pub fn extract_partitions(
    source_path: impl AsRef<Path>,
    dest_path: impl AsRef<Path>,
) -> Result<(), String> {
    if std::fs::exists(&dest_path).unwrap() {
        std::fs::remove_dir_all(&dest_path)
            .map_err(|e| format!("Error deleting output directory: {}", e))?;
    }

    std::fs::create_dir(&dest_path)
        .map_err(|e| format!("Error creating output directory: {}", e))?;

    let file = File::open(source_path).map_err(|e| format!("Error opening input file: {}", e))?;
    let mut reader = BufReader::new(file);

    const LBA_SIZE: u64 = 0x0200;

    let mut out_path = PathBuf::from(dest_path.as_ref());
    let mut buffer = vec![0u8; LBA_SIZE as usize];

    {
        out_path.push("0_ppt.bin");
        let mut ppt = File::create(&out_path).map_err(|e| format!("Error creating file: {}", e))?;
        reader
            .read_exact(&mut buffer)
            .map_err(|e| format!("Error reading file: {}", e))?;
        ppt.write_all(&buffer)
            .map_err(|e| format!("Error writing to file: {}", e))?;
        reader
            .read_exact(&mut buffer)
            .map_err(|e| format!("Error reading file: {}", e))?;
        ppt.write_all(&buffer)
            .map_err(|e| format!("Error writing to file: {}", e))?;
        out_path.pop();
    }

    reader
        .seek(io::SeekFrom::Start(LBA_SIZE))
        .map_err(|e| format!("Error seeking in input file: {}", e))?;
    let gpt_head = GPTHeader::read(&mut reader).unwrap();

    for i in 1..=gpt_head.number_of_partition_entries {
        let efi_part_entry = EFIPartitionEntry::read(&mut reader).unwrap();
        let name = String::from_utf16(&efi_part_entry.partition_name).unwrap();
        out_path.push(format!(
            "{}_{}.bin",
            i,
            name.trim_end_matches(char::from(0))
        ));
        let mut part_file =
            File::create(&out_path).map_err(|e| format!("Error creating file: {}", e))?;
        let cur_pos = reader
            .stream_position()
            .map_err(|e| format!("Error getting stream position: {}", e))?;
        reader
            .seek(io::SeekFrom::Start(efi_part_entry.starting_lba * LBA_SIZE))
            .map_err(|e| format!("Error seeking input file: {}", e))?;
        for _lba in efi_part_entry.starting_lba..=efi_part_entry.ending_lba {
            reader
                .read_exact(&mut buffer)
                .map_err(|e| format!("Error reading from file: {}", e))?;
            part_file
                .write_all(&buffer)
                .map_err(|e| format!("Error writing to file: {}", e))?;
        }
        reader
            .seek(io::SeekFrom::Start(cur_pos))
            .map_err(|e| format!("Error seeking input file: {}", e))?;
        out_path.pop();
    }

    Ok(())
}
