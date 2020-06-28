use std::{env::{self, Args}, iter::Peekable};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub mod opt {
    use std::{env::Args,iter::Peekable};

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub fn named(args: &mut Peekable<Args>, name: &str) -> R<Option<String>> {
        match args.peek() {
            Some(arg) if arg == name => {
                args.next(); // discard peeked arg name
                let value = args.next()
                    .ok_or(format!("Not enough arguments provided: missing value for '{}'.", name))?;
                Ok(Some(value))
            },
            _ => Ok(None)
        }
    }

    pub fn flag(args: &mut Peekable<Args>, flag: &str) -> bool {
        match args.peek() {
            Some(arg) if arg == flag => {
                args.next(); // discard peeked arg name
                true
            },
            _ => false
        }
    }
}

pub fn get_all() -> Peekable<Args> {
    let mut args = env::args().peekable();
    args.next().unwrap(); // skip name of executable
    args
}

pub fn unnamed(args: &mut Peekable<Args>, info: &str) -> R<String> {
    args.next().ok_or(format!("Not enough arguments provided: missing {}.", info).into())
}

pub fn named(args: &mut Peekable<Args>, name: &str) -> R<String> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing '{}'.", name))?;
    if arg == name {
        args.next()
            .ok_or(format!("Not enough arguments provided: missing value for '{}'.", name).into())
    } else {
        Err(format!("Expecting argument '{}'. Got '{}' instead.", name, arg).into())
    }
}

pub fn flag(args: &mut Peekable<Args>, flag: &str) -> R<()> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing flag '{}'.", flag))?;
    if arg == flag {
        Ok(())
    } else {
        Err(format!("Expecting flag '{}'. Got '{}' instead.", flag, arg).into())
    }
}
