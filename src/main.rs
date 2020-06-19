use std::env;
use std::env::Args;
use std::error::Error;
use std::fs;
use std::io::prelude::*;
use flate2::read::ZlibDecoder;

type Res<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    fn run(args: &mut Args) -> Res<String> {
        let cmd = args.next()
            .ok_or("no command provided. available commands: init, cat-file.")?;
        match cmd.as_str() {
            "init" => init(),
            "cat-file" => cat_file(args),
            _ => Err(format!("unknown command '{}'.", cmd).into())
        }
    }

    let mut args = env::args();
    args.next();
    let exit_code = match run(&mut args) {
        Ok(msg) => {
            println!("{}", msg);
            0
        },
        Err(err) => {
            println!("error: {}", err);
            1
        }
    };
    std::process::exit(exit_code);
}

fn init() -> Res<String> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok("initialized git directory.".to_string())
}

fn cat_file(args: &mut Args) -> Res<String> {
    fn assert_prettyprint(args: &mut Args) -> Res<()> {
        let arg = args.next()
            .ok_or("not enough arguments provided for command cat-file. missing flag '-p'.")?;
        match arg.as_str() {
            "-p" => Ok(()),
            _ => Err("cat-file must be used with '-p'.")?
        }
    }

    fn validate_sha(args: &mut Args) -> Res<String> {
        let sha = args.next()
            .ok_or("not enough arguments provided for command cat-file. missing SHA.")?;
        if sha.len() < 3 {
            Err("provided SHA is invalid.")?
        } else {
            Ok(sha)
        }
    }

    fn parse_content(content: String) -> Res<(String, String)> {
        let mut split = content.split('\x00');
        let header = split.next().ok_or("object content could not be parsed.")?.to_string();
        let mut data = split.next().ok_or("object content could not be parsed.")?.to_string();
        data.pop().ok_or("object content could not be parsed.")?;
        Ok((header, data))
    }

    assert_prettyprint(args)?;
    let sha = validate_sha(args)?;

    let (dir, filename) = sha.split_at(2);
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::open(&path)
        .map_err(|e| format!("object '{}' not found. {}", path, e))?;

    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed)?;

    let (_, data) = parse_content(decompressed)?;

    Ok(data)
}
