use std::{
    cmp::max,
    env, env::Args,
    fs, fs::DirEntry,
    io::prelude::{Read, Write},
    iter, iter::Peekable,
    path::Path,
    str,
    time::{SystemTime, UNIX_EPOCH}
};
use bytes::Bytes;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use reqwest::blocking::Client;
use sha1::{Sha1, Digest};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    fn run(args: &mut Peekable<Args>) -> R<String> {
        let cmd = args.next().ok_or("No command provided.")?;
        match cmd.as_str() {
            "cat-file"    => cat_file(args),
            "commit-tree" => commit_tree(args),
            "hash-object" => hash_object(args),
            "init"        => init(),
            "ls-remote"   => ls_remote(args),
            "ls-tree"     => ls_tree(args),
            "write-tree"  => write_tree(),
            _ => Err(format!("Unknown command '{}'.", cmd).into())
        }
    }

    let mut args = env::args().peekable();
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

// commands

fn cat_file(args: &mut Peekable<Args>) -> R<String> {
    fn parse_blob_content<'a>(content: &'a String) -> R<(&'a str, &'a str)> {
        let mut split = content.split('\x00');
        let header = split.next().unwrap();
        let data = split.next().ok_or("Content of blob object could not be parsed.")?;
        Ok((header, data))
    }

    parse_arg_flag(args, "-p")?;
    let sha = parse_arg(args, "SHA")?;
    validate_sha(&sha)?;
    let file = open_object(&sha)?;
    let decompressed = inflate_utf8(file)?;
    let (_, data) = parse_blob_content(&decompressed)?;
    Ok(data.to_string())
}

fn commit_tree(args: &mut Peekable<Args>) -> R<String> {
    let tree = parse_arg(args, "SHA")?;
    validate_sha(&tree)?;
    let parent = parse_opt_arg_named(args, "-p")?;
    if parent.is_some() { validate_sha(&parent.clone().unwrap())?; }
    let msg = parse_arg_named(args, "-m")?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let committer = format!("bert2 <shuairan@gmail.com> {} +0000", timestamp);
    let parent = parent.map(|p| format!("parent {}\n", p)).unwrap_or(String::new());
    let data = format!("tree {}\n{}author {}\ncommitter {}\n\n{}\n", tree, parent, committer, committer, msg);
    let content = format!("commit {}\x00{}", data.len(), data);
    let sha = print_sha(&Sha1::digest(content.as_bytes()));
    let out_file = create_object(&sha)?;
    write_object(out_file, content.as_bytes())?;
    Ok(sha)
}

fn hash_object(args: &mut Peekable<Args>) -> R<String> {
    parse_arg_flag(args, "-w")?;
    let in_file = parse_arg(args, "file")?;
    let content = create_blob_from_file(Path::new(&in_file))?;
    let sha = print_sha(&Sha1::digest(content.as_bytes()));
    let out_file = create_object(&sha)?;
    write_object(out_file, content.as_bytes())?;
    Ok(sha)
}

fn init() -> R<String> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok("Initialized git directory.".to_string())
}

fn ls_tree(args: &mut Peekable<Args>) -> R<String> {
    fn parse_tree_content(content: &Vec<u8>) -> R<Vec<&str>> {
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

        fn get_name(entry: &str) -> R<&str> {
            entry.split(' ')
                .skip(1) // skip mode
                .next()  // get name
                .ok_or(format!("Unable to parse tree entry '{}'", entry).into())
        }

        let entries = iterate_tree(content)
            .skip(1) // skip header "tree <byte size>"
            .map(str::from_utf8)
            .map(|x| x.map(get_name)?)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    parse_arg_flag(args, "--name-only")?;
    let sha = parse_arg(args, "SHA")?;
    validate_sha(&sha)?;
    let file = open_object(&sha)?;
    let decompressed = inflate_binary(file)?;
    let data = parse_tree_content(&decompressed)?;
    Ok(data.join("\n") + "\n")
}

fn ls_remote(args: &mut Peekable<Args>) -> R<String> {
    fn pkt_lines(bytes: Bytes) -> impl Iterator<Item = String> {
        let mut bytes = bytes;
        iter::from_fn(move || {
            if bytes.len() == 0 { return None; }
            let len = str::from_utf8(&bytes[..4]).expect("Failed to read pkt-len.");
            let len = usize::from_str_radix(len, 16).expect("Failed to parse pkt-len.");
            let pkt_line = match len {
                0 => String::new(),
                _ => str::from_utf8(&bytes[4..len]).unwrap().to_string()
            };

            bytes = bytes.slice(max(len, 4)..);

            Some(pkt_line)
        })
    }

    fn parse_ref(pkt_line: String) -> R<(String, String)> {
        let _ref = pkt_line.split('\0').next().unwrap();
        let mut ref_parts = _ref.split(' ');
        let id = ref_parts.next().unwrap();
        let name = ref_parts.next().ok_or("Failed to parse ref pkt-line.")?.trim();
        Ok((id.to_string(), name.to_string()))
    }

    fn print_ref(_ref: R<(String, String)>) -> R<String> {
        _ref.map(|(id,name)| format!("{}\t{}", id, name))
    };

    parse_arg_flag(args, "--refs")?;
    let url = parse_arg(args, "repository URL")?;
    let url = [url.as_str(), "/info/refs?service=git-receive-pack"].concat();

    let http = Client::new();
    let bytes = http.get(&url).send()?.bytes()?;

    let refs = pkt_lines(bytes)
        .skip(2) // skip command & flush-pkt
        .take_while(|pkt_line| pkt_line != "")
        .map(parse_ref)
        .map(print_ref)
        .collect::<R<Vec<_>>>()?
        .join("\n");

    Ok(refs)
}

fn write_tree() -> R<String> {
    fn render_entry(entry: &DirEntry) -> R<Vec<u8>> {
        fn render_file(entry: &DirEntry) -> R<Vec<u8>> {
            let mode_and_name = format!("100644 {}\x00", name(entry));
            let content = create_blob_from_file(&entry.path())?;
            let sha = Sha1::digest(&content.as_bytes());
            let mut row = Vec::from(mode_and_name.as_bytes());
            row.extend_from_slice(&sha);
            Ok(row)
        }

        fn render_dir(entry: &DirEntry) -> R<Vec<u8>> {
            let mode_and_name = format!("040000 {}\x00", name(entry));
            let sha = write_tree(&entry.path())?;
            let mut row = Vec::from(mode_and_name.as_bytes());
            row.extend_from_slice(&sha);
            Ok(row)
        }

        let _type = entry.file_type()?;
        if _type.is_file() {
            render_file(entry)
        } else if _type.is_dir() {
            render_dir(entry)
        } else {
            Err(format!("Symbolic links ('{}') are not supported.", name(entry)).into())
        }
    }

    fn write_tree(path: &Path) -> R<[u8; 20]> {
        let mut dir_entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        dir_entries.sort_by_key(name);
        let tree_entries = dir_entries.iter()
            .filter(|e| name(e) != ".git")
            .map(render_entry)
            .collect::<Result<Vec<_>, _>>()?
            .concat();
        let header = format!("tree {}\x00", tree_entries.len());
        let content = [Vec::from(header.as_bytes()), tree_entries].concat();
        let sha = Sha1::digest(&content);
        let out_file = create_object(&print_sha(&sha))?;
        write_object(out_file, &content)?;
        Ok(sha.into())
    }

    let sha = write_tree(Path::new("."))?;
    Ok(print_sha(&sha))
}

// helper functions

fn parse_arg(args: &mut Peekable<Args>, info: &str) -> R<String> {
    args.next().ok_or(format!("Not enough arguments provided: missing {}.", info).into())
}

fn parse_arg_named(args: &mut Peekable<Args>, name: &str) -> R<String> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing '{}'.", name))?;
    if arg == name {
        args.next()
            .ok_or(format!("Not enough arguments provided: missing value for '{}'.", name).into())
    } else {
        Err(format!("Expecting argument '{}'. Got '{}' instead.", name, arg).into())
    }
}

fn parse_opt_arg_named(args: &mut Peekable<Args>, name: &str) -> R<Option<String>> {
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

fn parse_arg_flag(args: &mut Peekable<Args>, flag: &str) -> R<()> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing flag '{}'.", flag))?;
    if arg == flag {
        Ok(())
    } else {
        Err(format!("Expecting flag '{}'. Got '{}' instead.", flag, arg).into())
    }
}

fn validate_sha(sha: &str) -> R<()> {
    match sha.len() {
        40 => Ok(()),
        _  => Err("SHA does not have the required length of 40 characters.".into())
    }
}

fn open_object(sha: &str) -> R<fs::File> {
    let (dir, filename) = sha.split_at(2);
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::open(&path)
        .map_err(|e| format!("Failed to read object '{}'. {}", path, e))?;
    Ok(file)
}

fn inflate_utf8(file: fs::File) -> R<String> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed)
        .map_err(|e| format!("Unable to inflate file into `String`. {}", e))?;
    Ok(decompressed)
}

fn inflate_binary(file: fs::File) -> R<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| format!("Unable to inflate binary file. {}", e))?;
    Ok(decompressed)
}

fn create_blob_from_file(file: &Path) -> R<String> {
    let mut data = String::new();
    fs::File::open(file)?.read_to_string(&mut data)
        .map_err(|e| format!("Failed to read file '{}': {}.", file.to_string_lossy(), e))?;
    let header = format!("blob {}", &data.len());
    let content = format!("{}\x00{}", header, data);
    Ok(content)
}

fn print_sha(sha: &[u8]) -> String {
    sha.iter()
        .map(|byte| format!("{:02x}", byte))
        .fold(String::new(), |sha, hex| sha + &hex)
}

fn create_object(sha: &str) -> R<fs::File> {
    let (dir, filename) = sha.split_at(2);
    fs::create_dir_all(["./.git/objects/", dir].concat())?;
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::create(&path)
        .map_err(|e| format!("Failed to create object '{}'. {}", path, e))?;
    Ok(file)
}

fn write_object(object: fs::File, content: &[u8]) -> R<()> {
    let mut encoder = ZlibEncoder::new(object, Compression::default());
    encoder.write(content)?;
    Ok(())
}

fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().to_string()
}
