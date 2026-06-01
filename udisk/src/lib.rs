use fatfs::{format_volume, Dir, FatType, FileSystem, FormatVolumeOptions, FsOptions};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

pub fn extract(image_path: impl AsRef<Path>, dest_path: impl AsRef<Path>) -> Result<(), String> {
    let dest = dest_path.as_ref();
    if dest.exists() {
        fs::remove_dir_all(dest)
            .map_err(|e| format!("Failed to remove output directory: {}", e))?;
    }
    fs::create_dir_all(dest).map_err(|e| format!("Failed to create output directory: {}", e))?;

    let file = File::open(image_path).map_err(|e| format!("Failed to open image file: {}", e))?;
    let fs = FileSystem::new(file, FsOptions::new())
        .map_err(|e| format!("Failed to parse FAT filesystem: {}", e))?;
    let root = fs.root_dir();

    extract_dir(&root, dest)?;
    Ok(())
}

fn extract_dir(dir: &Dir<File>, dest_path: &Path) -> Result<(), String> {
    for entry_res in dir.iter() {
        let entry = entry_res.map_err(|e| format!("Error iterating directory: {}", e))?;
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let current_path = dest_path.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&current_path)
                .map_err(|e| format!("Failed to create sub-directory: {}", e))?;
            let sub_dir = entry.to_dir();
            extract_dir(&sub_dir, &current_path)?;
        } else {
            let mut fat_file = entry.to_file();
            let mut out_file = File::create(&current_path)
                .map_err(|e| format!("Failed to create output file {:?}: {}", current_path, e))?;
            let mut buffer = vec![0u8; 8192];
            loop {
                let bytes_read = fat_file
                    .read(&mut buffer)
                    .map_err(|e| format!("Failed to read FAT file {:?}: {:?}", name, e))?;
                if bytes_read == 0 {
                    break;
                }
                out_file
                    .write_all(&buffer[..bytes_read])
                    .map_err(|e| format!("Failed to write file {:?}: {}", name, e))?;
            }
        }
    }
    Ok(())
}

pub fn pack(
    folder_path: impl AsRef<Path>,
    image_path: impl AsRef<Path>,
    image_size: u64,
) -> Result<(), String> {
    // 1. Create a blank file of size `image_size` filled with zeros
    let mut image_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&image_path)
        .map_err(|e| format!("Failed to create/open FAT image file: {}", e))?;
    image_file
        .set_len(image_size)
        .map_err(|e| format!("Failed to set FAT image file size: {}", e))?;

    // 2. Format the image file as FAT16
    let options = FormatVolumeOptions::new().fat_type(FatType::Fat16);
    format_volume(&mut image_file, options)
        .map_err(|e| format!("Failed to format FAT16 volume: {}", e))?;

    // 3. Open the formatted filesystem
    let fs = FileSystem::new(image_file, FsOptions::new())
        .map_err(|e| format!("Failed to open formatted filesystem: {}", e))?;
    let root = fs.root_dir();

    // 4. Recursively copy files from the folder to the FAT filesystem
    pack_dir(folder_path.as_ref(), &root)?;

    Ok(())
}

fn pack_dir(folder_path: &Path, fat_dir: &Dir<File>) -> Result<(), String> {
    for entry_res in
        fs::read_dir(folder_path).map_err(|e| format!("Failed to read folder: {}", e))?
    {
        let entry = entry_res.map_err(|e| format!("Failed to read folder entry: {}", e))?;
        let path = entry.path();
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid filename encoding".to_string())?;

        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            let sub_fat_dir = fat_dir
                .create_dir(&name)
                .map_err(|e| format!("Failed to create FAT directory {:?}: {:?}", name, e))?;
            pack_dir(&path, &sub_fat_dir)?;
        } else {
            let mut fat_file = fat_dir
                .create_file(&name)
                .map_err(|e| format!("Failed to create FAT file {:?}: {:?}", name, e))?;
            let mut local_file = File::open(&path)
                .map_err(|e| format!("Failed to open local file {:?}: {}", path, e))?;

            let mut buffer = vec![0u8; 8192];
            loop {
                let bytes_read = local_file
                    .read(&mut buffer)
                    .map_err(|e| format!("Failed to read local file: {}", e))?;
                if bytes_read == 0 {
                    break;
                }
                fat_file
                    .write_all(&buffer[..bytes_read])
                    .map_err(|e| format!("Failed to write to FAT file: {}", e))?;
            }
        }
    }
    Ok(())
}
