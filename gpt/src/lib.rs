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

/// A single partition as described by a device's own GPT, resolved to
/// concrete byte offsets/sizes. Nothing about this is board-specific:
/// `name` and geometry always come from the GPT that ships in the image
/// being processed, never from a constant baked into this tool.
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub name: String,
    pub start_lba: u64,
    pub end_lba: u64,
    /// Byte offset of the partition, relative to the start of the file
    /// the GPT was read from (e.g. relative to `gpt.bin`).
    pub offset: u64,
    /// Partition size in bytes, inclusive of both LBA bounds.
    pub size: u64,
}

/// Read the GPT partition table from `source_path` and return every named
/// partition entry with its real offset/size. Any code that needs to know
/// "where is bootA / ROOTFS / UDISK" for a given dump should call this
/// instead of assuming a fixed layout, since partition count, order,
/// offsets and sizes all vary by device.
pub fn read_partition_table(source_path: impl AsRef<Path>) -> Result<Vec<PartitionInfo>, String> {
    const LBA_SIZE: u64 = 0x0200;

    let file = File::open(&source_path).map_err(|e| format!("Error opening input file: {}", e))?;
    let mut reader = BufReader::new(file);

    reader
        .seek(io::SeekFrom::Start(LBA_SIZE))
        .map_err(|e| format!("Error seeking in input file: {}", e))?;
    let gpt_head = GPTHeader::read(&mut reader)
        .map_err(|e| format!("Error reading GPT header: {}", e))?;

    let mut partitions = Vec::with_capacity(gpt_head.number_of_partition_entries as usize);
    for _ in 1..=gpt_head.number_of_partition_entries {
        let efi_part_entry = EFIPartitionEntry::read(&mut reader)
            .map_err(|e| format!("Error reading partition entry: {}", e))?;

        // An all-zero type GUID marks an unused entry slot; the number of
        // *populated* entries can be smaller than number_of_partition_entries.
        if efi_part_entry.partition_type_guid.iter().all(|b| *b == 0) {
            continue;
        }

        let name = String::from_utf16(&efi_part_entry.partition_name)
            .map_err(|e| format!("Invalid partition name in GPT: {}", e))?
            .trim_end_matches(char::from(0))
            .to_string();
        if name.is_empty() {
            continue;
        }

        let offset = efi_part_entry.starting_lba * LBA_SIZE;
        let size = (efi_part_entry.ending_lba - efi_part_entry.starting_lba + 1) * LBA_SIZE;
        partitions.push(PartitionInfo {
            name,
            start_lba: efi_part_entry.starting_lba,
            end_lba: efi_part_entry.ending_lba,
            offset,
            size,
        });
    }

    Ok(partitions)
}

/// Look up a partition by name in a table returned by `read_partition_table`.
pub fn find_partition<'a>(
    partitions: &'a [PartitionInfo],
    name: &str,
) -> Result<&'a PartitionInfo, String> {
    partitions
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("Partition '{}' not found in GPT", name))
}

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
