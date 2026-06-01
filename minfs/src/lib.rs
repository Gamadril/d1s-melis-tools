use std::path::Path;

use build::build_fs;
use extract::extract_fs;

mod build;
mod extract;
mod structs;

pub fn extract(image_path: impl AsRef<Path>, dest_path: impl AsRef<Path>) -> Result<(), String> {
    extract_fs(image_path, dest_path)
}

pub fn pack(folder_path: impl AsRef<Path>, image_path: impl AsRef<Path>) -> Result<(), String> {
    build_fs(folder_path, image_path)
}
