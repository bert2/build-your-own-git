use std::io::prelude::Read;
use flate2::read::ZlibDecoder;

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn inflate<T>(data: T) -> R<(Vec<u8>, u64)> where T : Read {
    let mut decoder = ZlibDecoder::new(data);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated)
        .map_err(|e| format!("Unable to inflate binary data. {}", e))?;
    Ok((inflated, decoder.total_in()))
}
