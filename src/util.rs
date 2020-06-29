#![allow(dead_code)]

use std::{fs::DirEntry, time::{SystemTime, UNIX_EPOCH}};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn print_bin(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| format!("{:08b}", b)).collect::<Vec<_>>().join(" | ")
}

pub fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().into_owned()
}

pub fn timestamp() -> R<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
