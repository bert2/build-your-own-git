use std::{convert::{Into, TryInto, TryFrom}, fmt::{Display, Error, Formatter}, result::Result};
use sha1::{Sha1, Digest};
use crate::util;

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(PartialEq,Eq,Hash,Debug)]
pub struct Sha(String);

impl Sha {
    pub fn value(&self) -> &str {
        let Sha(v) = self;
        v
    }

    pub fn to_bytes(&self) -> [u8; 20] {
        util::decode_hex(self.value())
            .expect(&format!("SHA to convert contained invalid value '{}'.", self.value()))
            .as_slice().try_into()
            .expect("Conversion from byte slice to byte array failed.")
    }

    pub fn generate(data: &[u8]) -> Sha {
        Sha(util::print_hex(&Sha1::digest(data)))
    }

    pub fn generate_raw(data: &[u8]) -> [u8; 20] {
        Sha1::digest(data).try_into()
            .expect("Failed to generate raw SHA.")
    }

    pub fn from_str(s: &str) -> R<Sha> {
        Sha::validate(s)?;
        Ok(Sha(s.to_string()))
    }

    pub fn from_string(s: String) -> R<Sha> {
        Sha::validate(&s)?;
        Ok(Sha(s))
    }

    pub fn from_bytes(b: &[u8]) -> R<Sha> {
        Sha::validate_bytes(&b)?;
        Ok(Sha(util::print_hex(b)))
    }

    pub fn validate(s: &str) -> R<()> {
        match s.len() {
            40 => Ok(()),
            _  => Err(format!("String '{}' is not a valid SHA. Expected 40 characters.", s).into())
        }
    }

    pub fn validate_bytes(b: &[u8]) -> R<()> {
        match b.len() {
            20 => Ok(()),
            _  => Err(format!("Array slice is not a valid SHA. Expected 20 bytes, but got {}.", b.len()).into())
        }
    }
}

impl TryFrom<String> for Sha {
    type Error = std::boxed::Box<dyn std::error::Error>;
    fn try_from(s: String) -> Result<Self, <Self as TryFrom<String>>::Error> {
        Sha::from_string(s)
    }
}

impl Into<String> for Sha {
    fn into(self) -> String {
        let Sha(v) = self;
        v
    }
}

impl Clone for Sha {
    fn clone(&self) -> Self {
        Sha::from_str(self.value())
            .expect(&format!("SHA to clone contained invalid value '{}'.", self.value()))
    }
}

impl Display for Sha {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", self.value())
    }
}
