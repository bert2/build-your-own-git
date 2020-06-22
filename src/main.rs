use std::{env, env::Args, fs, fs::DirEntry, io::prelude::*, iter, path::Path, str, time::{SystemTime, UNIX_EPOCH}};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha1::{Sha1, Digest};

type Res<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    fn run(args: &mut Args) -> Res<String> {
        let cmd = args.next().ok_or("No command provided.")?;
        match cmd.as_str() {
            "cat-file"    => cat_file(args),
            "commit-tree" => commit_tree(args),
            "hash-object" => hash_object(args),
            "init"        => init(),
            "ls-tree"     => ls_tree(args),
            "write-tree"  => write_tree(),
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

fn cat_file(args: &mut Args) -> Res<String> {
    fn parse_blob_content<'a>(content: &'a String) -> Res<(&'a str, &'a str)> {
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

fn commit_tree(args: &mut Args) -> Res<String> {
    let tree = parse_arg(args, "SHA")?;
    validate_sha(&tree)?;
    let parent = parse_arg_named(args, "-p")?;
    validate_sha(&parent)?;
    let msg = parse_arg_named(args, "-m")?;

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let committer = format!("bert2 <shuairan@gmail.com> {} +0000", timestamp);
    let data = format!("tree {}\nparent {}\nauthor {}\ncommitter {}\n\n{}\n", tree, parent, committer, committer, msg);
    let content = format!("commit {}\x00{}", data.len(), data).as_bytes();
    let sha = print_sha(&Sha1::digest(content));
    let out_file = create_object(&sha)?;
    write_object(out_file, content)?;
    Ok(sha)
}

fn hash_object(args: &mut Args) -> Res<String> {
    parse_arg_flag(args, "-w")?;
    let in_file = parse_arg(args, "file")?;
    let content = create_blob_from_file(Path::new(&in_file))?;
    let sha = print_sha(&Sha1::digest(content.as_bytes()));
    let out_file = create_object(&sha)?;
    write_object(out_file, content.as_bytes())?;
    Ok(sha)
}

fn init() -> Res<String> {
    fs::create_dir(".git")?;
    fs::create_dir(".git/objects")?;
    fs::create_dir(".git/refs")?;
    fs::write(".git/HEAD", "ref: refs/heads/master\n")?;
    Ok("Initialized git directory.".to_string())
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

        fn get_name(entry: &str) -> Res<&str> {
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

fn write_tree() -> Res<String> {
    fn render_entry(entry: &DirEntry) -> Res<Vec<u8>> {
        fn render_file(entry: &DirEntry) -> Res<Vec<u8>> {
            let mode_and_name = format!("100644 {}\x00", name(entry));
            let content = create_blob_from_file(&entry.path())?;
            let sha = Sha1::digest(&content.as_bytes());
            let mut row = Vec::from(mode_and_name.as_bytes());
            row.extend_from_slice(&sha);
            Ok(row)
        }

        fn render_dir(entry: &DirEntry) -> Res<Vec<u8>> {
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

    fn write_tree(path: &Path) -> Res<[u8; 20]> {
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

fn parse_arg(args: &mut Args, info: &str) -> Res<String> {
    args.next().ok_or(format!("Not enough arguments provided: missing {} argument.", info))
}

fn parse_arg_named(args: &mut Args, name: &str) -> Res<String> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing '{}'.", name))?;
    if arg == name {
        args.next()
            .ok_or(format!("Not enough arguments provided: missing value for '{}'.", name))
    } else {
        Err(format!("Expecting argument '{}'. Got '{}' instead.", name, arg).into())
    }
}

fn parse_arg_flag(args: &mut Args, flag: &str) -> Res<()> {
    let arg = args.next()
        .ok_or(format!("Not enough arguments provided: missing flag '{}'.", flag))?;
    if arg == flag {
        Ok(())
    } else {
        Err(format!("Expecting flag '{}'. Got '{}' instead.", flag, arg).into())
    }
}

fn validate_sha(sha: &str) -> Res<()> {
    match sha.len() {
        40 => Ok(()),
        _  => Err("SHA does not have the required length of 40 characters.".into())
    }
}

fn open_object(sha: &str) -> Res<fs::File> {
    let (dir, filename) = sha.split_at(2);
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::open(&path)
        .map_err(|e| format!("Failed to read object '{}'. {}", path, e))?;
    Ok(file)
}

fn inflate_utf8(file: fs::File) -> Res<String> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed)
        .map_err(|e| format!("Unable to inflate file into `String`. {}", e))?;
    Ok(decompressed)
}

fn inflate_binary(file: fs::File) -> Res<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(file);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| format!("Unable to inflate binary file. {}", e))?;
    Ok(decompressed)
}

fn create_blob_from_file(file: &Path) -> Res<String> {
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

fn create_object(sha: &str) -> Res<fs::File> {
    let (dir, filename) = sha.split_at(2);
    fs::create_dir_all(["./.git/objects/", dir].concat())?;
    let path = ["./.git/objects/", dir, "/", filename].concat();
    let file = fs::File::create(&path)
        .map_err(|e| format!("Failed to create object '{}'. {}", path, e))?;
    Ok(file)
}

fn write_object(object: fs::File, content: &[u8]) -> Res<()> {
    let mut encoder = ZlibEncoder::new(object, Compression::default());
    encoder.write(content)?;
    Ok(())
}

fn name(entry: &DirEntry) -> String {
    entry.file_name().to_string_lossy().to_string()
}
