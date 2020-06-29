mod arg;
mod obj;
mod pack;
mod repo;
mod sha;
mod util;
mod wtree;
mod zlib;

use std::{convert::{TryInto, TryFrom}, env::Args, iter::{self, Peekable}, path::Path, str};
use bytes::buf::Buf;
use reqwest::blocking::Client;
use obj::ObjType;
use pack::{fmt::RawObj, http::Ref};
use zlib::inflate;

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug)]
pub enum EntryType {
    ObjCommit   = 1,
    ObjTree     = 2,
    ObjBlob     = 3,
    ObjTag      = 4,
    ObjOfsDelta = 6,
    ObjRefDelta = 7
}

impl TryFrom<u8> for EntryType {
    type Error = String;
    fn try_from(val: u8) -> Result<Self, Self::Error> {
        match val {
            1 => Ok(EntryType::ObjCommit),
            2 => Ok(EntryType::ObjTree),
            3 => Ok(EntryType::ObjBlob),
            4 => Ok(EntryType::ObjTag),
            6 => Ok(EntryType::ObjOfsDelta),
            7 => Ok(EntryType::ObjRefDelta),
            _ => Err(format!("Unknown entry type {}.", val).into())
        }
    }
}

impl TryFrom<EntryType> for obj::ObjType {
    type Error = String;
    fn try_from(entry_type: EntryType) -> Result<Self, Self::Error> {
        match entry_type {
            EntryType::ObjCommit   => Ok(ObjType::Commit),
            EntryType::ObjTree     => Ok(ObjType::Tree),
            EntryType::ObjBlob     => Ok(ObjType::Blob),
            EntryType::ObjTag      => Ok(ObjType::Tag),
            EntryType::ObjOfsDelta |
            EntryType::ObjRefDelta => Err(format!("Entry type {:?} is not a proper object type.", entry_type).into())
        }
    }
}

fn main() {
    fn run(args: &mut Peekable<Args>) -> R<String> {
        let cmd = args.next().ok_or("No command provided.")?;
        match cmd.as_str() {
            "cat-file"    => cat_file(args),
            "checkout"    => checkout(args),
            "clone"       => clone(args),
            "commit-tree" => commit_tree(args),
            "hash-object" => hash_object(args),
            "init"        => init(),
            "ls-remote"   => ls_remote(args),
            "ls-tree"     => ls_tree(args),
            "write-tree"  => write_tree(),
            _ => Err(format!("Unknown command '{}'.", cmd).into())
        }
    }

    let mut args = arg::get_all();
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

fn cat_file(args: &mut Peekable<Args>) -> R<String> {
    arg::flag(args, "-p")?;
    let sha = arg::unnamed(args, "object id")?;
    sha::validate(&sha)?;
    let content = obj::read_utf8(Path::new(".git"), &sha)?;
    let (_, data) = obj::blob::parse(&content)?;
    Ok(data.to_string())
}

fn checkout(args: &mut Peekable<Args>) -> R<String> {
    let commit = arg::unnamed(args, "commit id")?;
    sha::validate(&commit)?;
    let git_dir = Path::new("./.git");
    wtree::checkout_commit(git_dir, &commit)?;
    Ok(format!("HEAD is now at {}.", commit))
}

fn clone(args: &mut Peekable<Args>) -> R<String> {
    let url = arg::unnamed(args, "repository URL")?;
    let dir = arg::unnamed(args, "target directory")?;
    let http = Client::new();

    println!("Cloning into '{}'...", dir);
    let git_dir = repo::init(Path::new(&dir))?;
    println!("Receiving objects...");
    let (head, mut pack) = pack::http::clone(&http, &url)?;
    let obj_cnt = pack::fmt::parse_header(&mut pack)? as usize;
    println!("Unpacking {} objects...", obj_cnt);

    fn has_no_cont_bit(byte: &u8) -> bool { (byte & 0b10000000) == 0 }
    let mut objs = std::collections::HashMap::new();

    loop {
        let first = pack.first();
        if let None = first { break; }

        let obj_type: EntryType = ((first.unwrap() & 0b01110000) >> 4).try_into()?;
        let obj_start = pack.iter().position(has_no_cont_bit).ok_or("Never ending variable sized integer.")?;
        let _obj_props = pack.split_to(obj_start + 1);

        let deflated_len = match obj_type {
            EntryType::ObjCommit |
            EntryType::ObjTree |
            EntryType::ObjBlob => {
                let (content, deflated_len) = inflate::bytes(pack.as_ref())?;
                let obj = RawObj { obj_type: obj_type.try_into()?, content };
                let id = obj::write_gen(&git_dir, obj.obj_type, &obj.content)?;
                objs.insert(id, obj);
                Ok(deflated_len)
            },
            EntryType::ObjRefDelta => {
                let base_id = sha::print(&pack.split_to(20));
                let (delta, deflated_len) = inflate::bytes(pack.as_ref())?;

                match objs.get(&base_id) {
                    Some(base) => {
                        let content = pack::fmt::undeltify(delta, &base.content)?;
                        let obj = RawObj { obj_type: base.obj_type, content };
                        let id = obj::write_gen(&git_dir, obj.obj_type, &obj.content)?;
                        objs.insert(id, obj);
                    },
                    None => return Err(format!("Found delta referencing unknown base object {}.", base_id).into())
                }

                Ok(deflated_len)
            },
            _ => Err(format!("Unsupported entry type {:?}", obj_type))
        }?;

        pack.advance(deflated_len.try_into()?);
    }

    if objs.len() != obj_cnt {
        return Err(format!("Expected {} objects in pack file but found {}.", obj_cnt, objs.len()).into());
    }

    println!("Checking out HEAD {}...", head.name);
    wtree::checkout_commit(&git_dir, &head.id)?;

    Ok(String::from("...done."))
}

fn commit_tree(args: &mut Peekable<Args>) -> R<String> {
    let tree = arg::unnamed(args, "SHA")?;
    sha::validate(&tree)?;
    let parent = arg::opt::named(args, "-p")?;
    if let Some(p) = parent.as_ref() { sha::validate(p)?; }
    let msg = arg::named(args, "-m")?;

    let content = obj::commit::new(&tree, parent, &msg, "bert2", "shuaira@gmail.com")?;
    let id = sha::print_from_str(&content);
    obj::write_str(Path::new(".git"), &id, &content)?;
    Ok(id)
}

fn hash_object(args: &mut Peekable<Args>) -> R<String> {
    arg::flag(args, "-w")?;
    let in_file = arg::unnamed(args, "file")?;
    let content = obj::blob::from_file(Path::new(&in_file))?;
    let id = sha::print_from_str(&content);
    obj::write_str(Path::new(".git"), &id, &content)?;
    Ok(id)
}

fn init() -> R<String> {
    let git_dir = repo::init(Path::new("."))?;
    Ok(format!("Initialized empty Git repository in {:?}.", git_dir))
}

fn ls_remote(args: &mut Peekable<Args>) -> R<String> {
    let refs_only = arg::opt::flag(args, "--refs");
    let url = arg::unnamed(args, "repository URL")?;
    let http = Client::new();
    let refs = pack::http::get_advertised_refs(&http, &url, refs_only)?
        .iter()
        .map(|Ref {id, name}| format!("{}\t{}", id, name))
        .collect::<Vec<_>>();
    Ok(refs.join("\n"))
}

fn ls_tree(args: &mut Peekable<Args>) -> R<String> {
    fn parse_tree_content(content: &[u8]) -> R<Vec<&str>> {
        fn iterate_tree(bytes: &[u8]) -> R<impl Iterator<Item = &[u8]>> {
            let mut parts = bytes.splitn(2, |&b| b == 0);
            parts.next()
                .filter(|&header| header.starts_with(b"tree "))
                .ok_or("Object is not a tree.")?;
            let mut entries = parts.next().ok_or("Failed to parse tree object: no entries found.")?;
            let sha_len = 20;

            Ok(iter::from_fn(move || {
                let utf8_end = entries.iter().position(|&b| b == 0)?;
                let next = &entries[..utf8_end];
                entries = &entries[utf8_end + 1 + sha_len ..];
                Some(next)
            }))
        }

        fn get_name(entry: &str) -> R<&str> {
            entry.split(' ')
                .skip(1) // skip mode
                .next()  // get name
                .ok_or(format!("Unable to parse tree entry '{}'", entry).into())
        }

        let entries = iterate_tree(content)?
            .map(str::from_utf8)
            .flat_map(|x| x.map(get_name))
            .collect::<R<Vec<_>>>()?;
        Ok(entries)
    }

    arg::flag(args, "--name-only")?;
    let sha = arg::unnamed(args, "SHA")?;
    sha::validate(&sha)?;
    let content = obj::read(Path::new("./.git"), &sha)?;
    let data = parse_tree_content(&content)?;
    Ok(data.join("\n") + "\n")
}

fn write_tree() -> R<String> {
    let sha = wtree::write(Path::new(".git"), Path::new("."))?;
    Ok(sha::print(&sha))
}
