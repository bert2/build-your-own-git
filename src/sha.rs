use sha1::{Sha1, Digest};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn from(data: &[u8]) -> [u8; 20] { Sha1::digest(data).into() }

pub fn print(sha: &[u8]) -> String {
    if sha.len() != 20 { panic!("SHA does not have the required length."); }
    sha.iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<Vec<_>>()
        .concat()
}

pub fn print_from(data: &[u8]) -> String { print(&from(data)) }

pub fn validate(sha: &str) -> R<()> {
    match sha.len() {
        40 => Ok(()),
        _  => Err("SHA does not have the required length of 40 characters.".into())
    }
}
