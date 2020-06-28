#![allow(dead_code)]

use std::fs::DirEntry;

pub fn print_bin(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| format!("{:08b}", b)).collect::<Vec<_>>().join(" | ")
}

pub fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().into_owned()
}