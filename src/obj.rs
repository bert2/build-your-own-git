use std::{convert::{TryInto, TryFrom}, fs::{self, File}, iter, path::Path, str::{self, FromStr}};
use crate::{util, sha::Sha, zlib};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub enum Obj {
    Commit {
        tree: Sha,
        parent: Option<Sha>,
        author: String,
        committer: String,
        message: String
    },
    Tree { entries: Vec<TreeEntry> },
    Blob { content: Vec<u8> }
}

#[derive(Clone,Copy,Debug)]
pub enum ObjType {
    Commit,
    Tree,
    Blob,
    Tag
}

impl ObjType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjType::Commit => "commit",
            ObjType::Tree   => "tree",
            ObjType::Blob   => "blob",
            ObjType::Tag    => panic!("Unsupported object type {:?}.", self),
        }
    }
}

#[derive(Clone,Debug)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: String,
    pub id: Sha,
}

impl TreeEntry {
    pub fn print_type(&self) -> &'static str {
        match self.mode {
            040000 => "tree",
            100644 |
            100755 => "blob",
            mode   => panic!("Unsupported file mode: {}", mode)
        }
    }
}

impl TryFrom<&str> for ObjType {
    type Error = String;
    fn try_from(obj_type: &str) -> Result<Self, Self::Error> {
        match obj_type {
            "commit" => Ok(ObjType::Commit),
            "tree"   => Ok(ObjType::Tree),
            "blob"   => Ok(ObjType::Blob),
            "tag"    => Ok(ObjType::Tag),
            _        => Err(format!("Unkown object type {:?}.", obj_type).into())
        }
    }
}

pub fn create(git_dir: &Path, id: &Sha) -> R<File> {
    let (dir, filename) = id.value().split_at(2);
    let obj_dir = git_dir.join("objects").join(dir);
    fs::create_dir_all(&obj_dir)?;
    let path = obj_dir.join(filename);
    let file = fs::File::create(&path)
        .map_err(|e| format!("Failed to create object {:?}. {}", path, e))?;
    Ok(file)
}

pub fn open(git_dir: &Path, id: &Sha) -> R<File> {
    let (dir, filename) = id.value().split_at(2);
    let path = git_dir.join("objects").join(dir).join(filename);
    let file = File::open(&path)
        .map_err(|e| format!("Failed to read object {:?}. {}", path, e))?;
    Ok(file)
}

pub fn read(git_dir: &Path, id: &Sha) -> R<Obj> {
    fn parse_commit(bytes: &mut Vec<u8>) -> R<Obj> {
        fn parse_line(label: &str, bytes: &mut Vec<u8>) -> R<String> {
            let end = bytes.iter().position(|&b| b == b'\n').ok_or("Unexpected EOF.")?;
            let line = &bytes[..end];
            let mut line = line.splitn(2, |&b| b == b' ');

            let line_lbl = str::from_utf8(line.next().unwrap())
                .map_err(|e| format!("Failed to parse label of commit {}: {}", label, e))?;
            if line_lbl != label {
                return Err(format!("Expected label '{}'. Got '{}' instead.", label, line_lbl).into());
            }

            let data = line.next().ok_or(format!("Missing data for commit {}.", label))?;
            let data = str::from_utf8(data)
                .map_err(|e| format!("Invalid data in commit {}: {}", label, e))?
                .to_string();

            bytes.drain(..end+1);

            Ok(data)
        }

        let tree = Sha::from_string(parse_line("tree", bytes)?)?;
        let parent = Some(Sha::from_string(parse_line("parent", bytes)?)?);
        let author = parse_line("author", bytes)?;
        let committer = parse_line("committer", bytes)?;
        let message = str::from_utf8(&bytes[1 .. bytes.len()-1])
            .map_err(|e| format!("Invalid data in commit message: {}", e))?
            .to_string();

        Ok(Obj::Commit { tree, parent, author, committer, message })
    }

    fn parse_tree(bytes: &[u8]) -> R<Obj> {
        fn iterate_tree(mut bytes: &[u8]) -> impl Iterator<Item = R<TreeEntry>> + '_ {
            let id_len = 20;

            iter::from_fn(move || {
                let utf8_end = bytes.iter().position(|&b| b == 0)?;

                let mode_name = &bytes[..utf8_end];
                let mut mode_name = mode_name.split(|&b| b == b' ');

                let mode = mode_name.next().unwrap();
                let mode = str::from_utf8(mode);
                if let Err(e) = mode { return Some(Err(format!("Invalid mode in tree entry: {}", e).into())) }
                let mode = u32::from_str(mode.unwrap());
                if let Err(e) = mode { return Some(Err(format!("Invalid mode in tree entry: {}", e).into())) }
                let mode = mode.unwrap();

                let name = mode_name.next();
                if let None = name { return Some(Err("Missing filename in tree entry.".into())); }
                let name = str::from_utf8(name.unwrap());
                if let Err(e) = name { return Some(Err(format!("Invalid filename in tree entry: {}", e).into())) }
                let name = name.unwrap().to_string();

                let id = &bytes[utf8_end+1 .. utf8_end+1+id_len];
                let id = Sha::from_bytes(id);
                if let Err(e) = id { return Some(Err(format!("Invalid SHA in tree entry: {}", e).into())) }

                bytes = &bytes[utf8_end+1+id_len ..];
                Some(Ok(TreeEntry { id: id.unwrap(), mode, name }))
            })
        }

        let entries = iterate_tree(bytes).collect::<R<Vec<_>>>()?;

        Ok(Obj::Tree { entries })
    }

    let (mut bytes, _) = zlib::inflate(open(git_dir, id)?)?;
    let header_end = bytes.iter().position(|&b| b == 0)
        .ok_or(format!("Object {} has no header.", id))?;

    let header = &bytes[..header_end];
    let mut header = header.splitn(2, |&b| b == b' ');

    let obj_type = str::from_utf8(header.next().unwrap())
        .map_err(|e| format!("Invalid type spec in header of object {}: {}", id, e))?;
    let obj_type: ObjType = obj_type.try_into()?;

    let obj_size = header.next().ok_or(format!("Missing size spec in header of object {}.", id))?;
    let obj_size = str::from_utf8(obj_size)
        .map_err(|e| format!("Invalid size spec in header of object {}: {}", id, e))?;
    let obj_size = usize::from_str(obj_size)?;

    bytes.drain(..header_end+1);

    if bytes.len() != obj_size {
        return Err(format!("Expected ({}) and actual ({}) size of object {} don't match.", bytes.len(), obj_size, id).into());
    }

    match obj_type {
        ObjType::Commit => parse_commit(&mut bytes),
        ObjType::Tree   => parse_tree(&bytes),
        ObjType::Blob   => Ok(Obj::Blob { content: bytes }),
        _               => Err(format!("Unsupported object type {:?}", obj_type).into())
    }
}

pub fn write(git_dir: &Path, obj_type: ObjType, content: &[u8]) -> R<Sha> {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(obj_type.as_str().as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(format!("{}", content.len()).as_bytes());
    bytes.push(b'\0');
    bytes.extend_from_slice(content);

    let id = Sha::generate(&bytes);
    let file = create(git_dir, &id)?;
    zlib::deflate(&bytes, file)?;

    Ok(id)
}

pub fn print(obj: &Obj) -> String {
    fn print_commit(tree: &Sha, parent: &Option<Sha>, author: &str, committer: &str, message: &str) -> String {
        let mut commit = String::new();

        commit.push_str(&format!("tree {}\n", tree));
        if let Some(p) = parent { commit.push_str(&format!("parent {}\n", p)); }
        commit.push_str(&format!("author {}\n", author));
        commit.push_str(&format!("committer {}\n", committer));
        commit.push_str("\n");
        commit.push_str(message);
        commit.push_str("\n");

        commit
    }

    fn print_tree(entries: &Vec<TreeEntry>) -> String {
        entries.iter()
            .map(|e| format!("{:06} {} {}    {}\n", e.mode, e.print_type(), e.id, e.name))
            .collect::<Vec<_>>()
            .concat()
    }

    match obj {
        Obj::Blob { content }
            => String::from_utf8_lossy(&content).to_string(),
        Obj::Commit { tree, parent, author, committer, message }
            => print_commit(tree, parent, author, committer, message),
        Obj::Tree { entries }
            => print_tree(&entries)
    }
}

pub fn print_commit_author(user: &str, email: &str) -> R<String> {
    Ok(format!("{} <{}> {} +0000", user, email, util::timestamp()?))
}
