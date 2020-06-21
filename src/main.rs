use std::{env, env::Args, fs, io::prelude::*, iter, str};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Sha1, Digest};

type Res<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    fn run(args: &mut Args) -> Res<String> {
        let cmd = args.next().ok_or("No command provided.")?;
        match cmd.as_str() {
            "cat-file"    => cat_file(args),
            "hash-object" => hash_object(args),
            "init"        => init(),
            "ls-tree"     => ls_tree(args),
            _ => Err(format!("Unknown command '{}'.", cmd).into())
        }
    }

    let mut args = env::args();
    args.next(); // skip name of executable

    let exit_code = match run(&mut args) {
        Ok(msg) => {
            print!("{}", msg);
            0
        },
        Err(err) => {
            println!("ERROR: {}", err);
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
    Ok("Initialized git directory.".to_string())
}

fn cat_file(args: &mut Args) -> Res<String> {
    fn parse_blob_content<'a>(content: &'a String) -> Res<(&'a str, &'a str)> {
        let mut split = content.split('\x00');
        let header = split.next().unwrap();
        let data = split.next().ok_or("Content of blob object could not be parsed.")?;
        Ok((header, data))
    }

    expect_arg_flag(args, "-p")?;
    let sha = expect_arg_sha(args)?;
    let file = open_object(&sha)?;
    let decompressed = decompress_utf8(file)?;
    let (_, data) = parse_blob_content(&decompressed)?;
    Ok(data.to_string())
}

fn hash_object(args: &mut Args) -> Res<String> {
    fn create_content_from_file(file: &str) -> Res<String> {
        let mut data = String::new();
        fs::File::open(file)?.read_to_string(&mut data)
            .map_err(|e| format!("Failed to read file '{}'. {}", file, e))?;
        let header = format!("blob {}", &data.len());
        let content = format!("{}\x00{}", header, data);
        Ok(content)
    }

    fn compute_sha(content: &str) -> Res<String> {
        let sha =  Sha1::digest(content.as_bytes()).iter()
            .map(|byte| format!("{:02x}", byte))
            .fold(String::new(), |sha, hex| sha + &hex);
        if sha.len() != 40 {
            Err(format!("Generated SHA '{}' is invalid.", sha).into())
        } else {
            Ok(sha)
        }
    }

    fn create_object(sha: &str) -> Res<fs::File> {
        let (dir, filename) = sha.split_at(2);
        fs::create_dir_all(["./.git/objects/", dir].concat())?;
        let path = ["./.git/objects/", dir, "/", filename].concat();
        let file = fs::File::create(&path)
            .map_err(|e| format!("Failed to create object '{}'. {}", path, e))?;
        Ok(file)
    }

    fn write_object(object: fs::File, content: &str) -> Res<()> {
        let mut encoder = ZlibEncoder::new(object, Compression::default());
        encoder.write(content.as_bytes())?;
        Ok(())
    }

    expect_arg_flag(args, "-w")?;
    let in_file = args.next().ok_or("Missing file argument.")?;
    let content = create_content_from_file(&in_file)?;
    let sha = compute_sha(&content)?;
    let out_file = create_object(&sha)?;
    write_object(out_file, &content)?;
    Ok(sha)
}

fn ls_tree(args: &mut Args) -> Res<String> {
    fn parse_tree_content(content: &Vec<u8>) -> Res<Vec<&str>> {
        fn iterate_tree(bytes: &Vec<u8>) -> impl Iterator<Item = &[u8]> {
            let mut bytes = bytes.as_slice();
            let mut sha_len = 0; // header has no SHA

            iter::from_fn(move || {
                let utf8_end = bytes.iter().position(|&x| x == 0)?;
                let next = &bytes[..utf8_end];
                bytes = &bytes[utf8_end + 1 + sha_len ..];
                sha_len = 20; // skip 20 byte SHA of subsequent entries
                Some(next)
            })
        }

        let entries = iterate_tree(content)
            .skip(1)    // skip header "tree <byte size>"
            .map(str::from_utf8)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    expect_arg_flag(args, "--name-only")?;
    let sha = expect_arg_sha(args)?;
    let file = open_object(&sha)?;
    let decompressed = decompress_binary(file)?;
    let data = parse_tree_content(&decompressed)?;
    Ok(data.join("\n"))
}

// helper functions

fn expect_arg_flag(args: &mut Args, flag: &str) -> Res<()> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing flag '{}'.", flag))?;
    if arg == flag {
        Ok(())
    } else {
        Err(format!("Expecting flag '--{}'. Got '{}' instead.", flag, arg).into())
    }
}

fn expect_arg_sha(args: &mut Args) -> Res<String> {
    let sha = args.next().ok_or("Missing SHA argument.")?;
    match sha.len() {
        40 => Ok(sha),
        _  => Err("Provided SHA does not have the required length of 40 characters.".into())
    }
}

fn open_object(sha: &str) -> Res<fs::File> {
    let (dir, filename) = sha.split_at(2);
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::open(&path)
        .map_err(|e| format!("Failed to read object '{}'. {}", path, e))?;
    Ok(file)
}

fn decompress_utf8(file: fs::File) -> Res<String> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed)
        .map_err(|e| format!("Unable to decompress file into `String`. {}", e))?;
    Ok(decompressed)
}

fn decompress_binary(file: fs::File) -> Res<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| format!("Unable to decompress binary file. {}", e))?;
    Ok(decompressed)
}
