use std::io::prelude::{Read, Write};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn inflate<T>(input: T) -> R<(Vec<u8>, u64)>
where T : Read {
    let mut decoder = ZlibDecoder::new(input);
    let mut inflated = Vec::new();
    decoder.read_to_end(&mut inflated)?;
    Ok((inflated, decoder.total_in()))
}

pub fn deflate<T>(input: &[u8], output: T) -> R<()>
where T : Write {
    let mut encoder = ZlibEncoder::new(output, Compression::default());
    encoder.write(input)?;
    Ok(())
}
