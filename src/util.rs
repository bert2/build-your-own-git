#![allow(dead_code)]

use std::{fs::{DirEntry, File}, io::Read, path::Path};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn read_file<P: AsRef<Path>>(path: P) -> R<Vec<u8>> {
    let mut file = File::open(&path)
        .map_err(|e| format!("Failed to open file '{}': {}.", path.as_ref().to_string_lossy(), e))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read file '{}': {}.", path.as_ref().to_string_lossy(), e))?;

    Ok(bytes)
}

pub fn print_bin(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| format!("{:08b}", b)).collect::<Vec<_>>().join(" | ")
}

pub fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().into_owned()
}