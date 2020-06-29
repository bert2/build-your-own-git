mod arg;
mod obj;
mod pack;
mod repo;
mod sha;
mod util;
mod wtree;
mod zlib;

use std::{env::Args, iter::Peekable, path::Path};
use reqwest::blocking::Client;
use obj::{Obj, ObjType};
use pack::http::Ref;
use sha::Sha;

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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

    let x = 1;
    println!("=== startup ==============");

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
    let id = Sha::from_string(arg::unnamed(args, "object id")?)?;
    let obj = obj::read(&repo::git_dir()?, &id)?;
    let output = obj::print(&obj);
    Ok(output)
}

fn checkout(args: &mut Peekable<Args>) -> R<String> {
    let commit = Sha::from_string(arg::unnamed(args, "commit id")?)?;
    wtree::checkout(&repo::git_dir()?, &commit)?;
    Ok(format!("HEAD is now at {}.", commit))
}

fn clone(args: &mut Peekable<Args>) -> R<String> {
    let url = arg::unnamed(args, "repository URL")?;
    let dir = arg::unnamed(args, "target directory")?;

    println!("Cloning into '{}'...", dir);
    let git_dir = repo::init(Path::new(&dir))?;

    println!("Receiving objects...");
    let http = Client::new();
    let (head, mut pack) = pack::http::clone(&http, &url)?;
    let expected_objs = pack::fmt::parse_header(&mut pack)? as usize;

    println!("Unpacking {} objects...", expected_objs);
    let objs = pack::fmt::unpack_objects(&git_dir, &mut pack)?;
    if objs != expected_objs {
        return Err(format!("Expected {} objects in pack file but found {}.", expected_objs, objs).into());
    }

    println!("Checking out HEAD {}...", head.name);
    wtree::checkout(&git_dir, &head.id)?;

    Ok("...done.".to_string())
}

fn commit_tree(args: &mut Peekable<Args>) -> R<String> {
    let tree = Sha::from_string(arg::unnamed(args, "SHA")?)?;
    let parent = arg::opt::named(args, "-p")?.map(Sha::from_string).transpose()?;
    let message = arg::named(args, "-m")?;

    let author = obj::print_commit_author("bert2", "shuairan@gmail.com")?;
    let committer = author.clone();
    let commit = Obj::Commit { tree, parent, author, committer, message };
    let commit = obj::print(&commit);
    let id = obj::write(&repo::git_dir()?, ObjType::Commit, commit.as_bytes())?;

    Ok(id.into())
}

fn hash_object(args: &mut Peekable<Args>) -> R<String> {
    arg::flag(args, "-w")?;
    let file = arg::unnamed(args, "file")?;
    let content = wtree::read_file(Path::new(&file))?;
    let id = obj::write(&repo::git_dir()?, ObjType::Blob, &content)?;
    Ok(id.into())
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
    let name_only = arg::opt::flag(args, "--name-only");
    let id = Sha::from_string(arg::unnamed(args, "SHA")?)?;
    let obj = obj::read(&repo::git_dir()?, &id)?;

    match obj {
        Obj::Tree { entries } if name_only =>
            Ok(entries.iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
                .join("\n")),
        Obj::Tree { .. } =>
            Ok(obj::print(&obj)),
        _ => Err(format!("Object {} is not a tree.", id).into())
    }
}

fn write_tree() -> R<String> {
    let id = wtree::write_tree(&repo::git_dir()?, Path::new("."))?;
    Ok(id.into())
}
