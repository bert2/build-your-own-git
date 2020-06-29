#![allow(dead_code)]

use std::{fs::DirEntry, time::{SystemTime, UNIX_EPOCH}};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn print_hex(bytes: &[u8]) -> String {
    bytes.iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<Vec<_>>()
        .concat()
}

pub fn decode_hex(s: &str) -> R<Vec<u8>> {
    if s.len() % 2 != 0 {
        Err("Input string must have an even length.".into())
    } else {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.into()))
            .collect()
    }
}

pub fn print_bin(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| format!("{:08b}", b)).collect::<Vec<_>>().join(" | ")
}

pub fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().into_owned()
}

pub fn timestamp() -> R<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
