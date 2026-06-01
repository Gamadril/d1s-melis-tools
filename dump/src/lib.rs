use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use binrw::{io::BufReader, BinRead};
use structs::*;

pub mod structs;

pub fn unpack_dump(dump_path: impl AsRef<Path>, dest_path: impl AsRef<Path>) -> Result<(), String> {
    if std::fs::exists(&dest_path).unwrap() {
        std::fs::remove_dir_all(&dest_path)
            .map_err(|e| format!("Error deleting output directory: {}", e))?;
    }
    std::fs::create_dir(&dest_path)
        .map_err(|e| format!("Error creating output directory: {}", e))?;

    let file = File::open(dump_path).map_err(|e| format!("Error opening input file: {}", e))?;
    let mut reader = BufReader::new(file);

    let head = BootloaderHead::read(&mut reader)
        .map_err(|e| format!("Error reading input file: {}", e))?;

    {
        let mut boot0_path = PathBuf::from(dest_path.as_ref());
        boot0_path.push("boot0.bin");

        reader.seek_invalidate(io::SeekFrom::Start(0)).unwrap();
        let mut boot0 =
            File::create(boot0_path).map_err(|e| format!("Error creating file: {}", e))?;
        let mut buffer = vec![0u8; head.length as usize];
        reader
            .read_exact(&mut buffer)
            .map_err(|e| format!("Error reading file: {}", e))?;
        boot0
            .write_all(&buffer)
            .map_err(|e| format!("Error writing file: {}", e))?;
    }

    const BUF_SIZE: usize = 0x0200;

    let mut fs_path = PathBuf::from(dest_path.as_ref());
    fs_path.push("gpt.bin");

    {
        let mut fs = File::create(&fs_path).map_err(|e| format!("Error creating file: {}", e))?;
        let mut buffer = vec![0u8; BUF_SIZE];
        loop {
            let n = reader
                .read(&mut buffer)
                .map_err(|e| format!("Error reading file: {}", e))?;
            if n != 0 {
                let _ = fs
                    .write(&buffer)
                    .map_err(|e| format!("Error writing to file: {}", e));
            } else {
                break;
            }
        }
    }

    Ok(())
}

