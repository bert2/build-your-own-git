use std::{env, env::Args, fs, io::prelude::*};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Sha1, Digest};

type Res<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    fn run(args: &mut Args) -> Res<String> {
        let cmd = args.next()
            .ok_or("no command provided. available commands: init, cat-file.")?;
        match cmd.as_str() {
            "init"        => init(),
            "cat-file"    => cat_file(args),
            "hash-object" => hash_object(args),
            _ => Err(format!("unknown command '{}'.", cmd).into())
        }
    }

    let mut args = env::args();
    args.next();
    let exit_code = match run(&mut args) {
        Ok(msg) => {
            print!("{}", msg);
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
            _ => Err(format!("cat-file expects flag '-p'. got '{}' instead.", arg))?
        }
    }

    fn validate_sha(args: &mut Args) -> Res<String> {
        let sha = args.next()
            .ok_or("not enough arguments provided for command cat-file. missing SHA.")?;
        if sha.len() != 40 {
            Err("provided SHA does not have the required length of 40 characters.")?
        } else {
            Ok(sha)
        }
    }

    fn parse_content(content: String) -> Res<(String, String)> {
        let mut split = content.split('\x00');
        let header = split.next().ok_or("object content could not be parsed.")?;
        let data = split.next().ok_or("object content could not be parsed.")?;
        Ok((header.to_string(), data.to_string()))
    }

    assert_prettyprint(args)?;
    let sha = validate_sha(args)?;

    let (dir, filename) = sha.split_at(2);
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::open(&path)
        .map_err(|e| format!("failed to read object '{}'. {}", path, e))?;

    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed)?;

    let (_, data) = parse_content(decompressed)?;

    Ok(data)
}

fn hash_object(args: &mut Args) -> Res<String> {
    fn assert_write(args: &mut Args) -> Res<()> {
        let arg = args.next()
            .ok_or("not enough arguments provided for command hash-object. missing flag '-w'.")?;
        match arg.as_str() {
            "-w" => Ok(()),
            _ => Err(format!("hash-object expects flag '-w'. got '{}' instead.", arg))?
        }
    }

    fn get_filename(args: &mut Args) -> Res<String> {
        let name = args.next()
            .ok_or("not enough arguments provided for command hash-object. missing file path.")?;
        Ok(name)
    }

    fn create_content(filename: &String) -> Res<String> {
        let mut in_data = String::new();
        fs::File::open(filename)?.read_to_string(&mut in_data)
            .map_err(|e| format!("failed to read file '{}'. {}", filename, e))?;
        let content = ["blob ", &in_data.len().to_string(), "\x00", &in_data].concat();
        Ok(content)
    }

    fn compute_sha1(content: &String) -> Res<String> {
        let sha_bytes = Sha1::digest(content.as_bytes());
        let sha: Vec<String> = sha_bytes.iter().map(|x| format!("{:x}", x)).collect();
        Ok(sha.concat())
    }

    assert_write(args)?;
    let in_filename = get_filename(args)?;

    let in_content = create_content(&in_filename)?;
    let sha = compute_sha1(&in_content)?;
    let (dir, out_filename) = sha.split_at(2);

    let out_filepath = ["./.git/objects/", dir, "/", out_filename].concat();
    let out_file = fs::File::create(&out_filepath)
        .map_err(|e| format!("failed to create blob '{}'. {}", out_filepath, e))?;
    let mut encoder = ZlibEncoder::new(out_file, Compression::default());
    encoder.write(in_content.as_bytes())?;

    Ok(sha)
}
