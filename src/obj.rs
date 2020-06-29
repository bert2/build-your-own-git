use std::{convert::{TryInto, TryFrom}, fs::{self, File}, io::prelude::Write, iter, path::Path, str::{self, FromStr}};
use flate2::{Compression, write::ZlibEncoder};
use crate::{sha, zlib};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub enum Obj {
    Commit { tree: String },
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

#[derive(Clone,Debug)]
pub struct TreeEntry {
    pub mode: u32,
    pub name: String,
    pub id: String,
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

pub mod blob {
    use std::{fs::File, io::prelude::Read, path::Path};

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub fn from_file(file: &Path) -> R<String> {
        let mut data = String::new();
        File::open(file)?.read_to_string(&mut data)
            .map_err(|e| format!("Failed to read file '{}': {}.", file.to_string_lossy(), e))?;
        let content = format!("blob {}\x00{}", &data.len(), data);
        Ok(content)
    }
}

pub mod commit {
    use std::time::{SystemTime, UNIX_EPOCH};

    type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub fn new(tree_id: &str, parent_id: Option<String>, msg: &str, user: &str, email: &str) -> R<String> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let committer = format!("{} <{}> {} +0000", user, email, timestamp);
        let parent = parent_id.map(|p| format!("parent {}\n", p)).unwrap_or(String::new());
        let data = format!("tree {}\n{}author {}\ncommitter {}\n\n{}\n", tree_id, parent, committer, committer, msg);
        let content = format!("commit {}\x00{}", data.len(), data);
        Ok(content)
    }
}

pub fn read_gen(git_dir: &Path, id: &str) -> R<Obj> {
    let (mut bytes, _) = zlib::inflate(open_file(git_dir, id)?)?;
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

pub fn print(obj: &Obj) -> String {
    fn print_commit(tree: &str) -> String {
        format!("tree {}\n", tree)
    }

    fn print_tree(entries: &Vec<TreeEntry>) -> String {
        entries.iter()
            .map(|e| format!("{:06} {} {}    {}", e.mode, e.print_type(), e.id, e.name))
            .collect::<Vec<_>>()
            .join("\n")
    }

    match obj {
        Obj::Blob { content } => String::from_utf8_lossy(&content).to_string(),
        Obj::Commit { tree }  => print_commit(tree),
        Obj::Tree { entries } => print_tree(&entries)
    }
}

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

    let tree = parse_line("tree", bytes)?;

    Ok(Obj::Commit { tree })
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
            let id = sha::print(id);

            bytes = &bytes[utf8_end+1+id_len ..];
            Some(Ok(TreeEntry { id, mode, name }))
        })
    }

    let entries = iterate_tree(bytes)
        .collect::<R<Vec<_>>>()?;

    Ok(Obj::Tree { entries })
}

pub fn write_gen(git_dir: &Path, obj_type: ObjType, content: &[u8]) -> R<String> {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(obj_type.as_str().as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(format!("{}", content.len()).as_bytes());
    bytes.push(b'\0');
    bytes.extend_from_slice(content);

    let id = sha::print_from(&bytes);
    write(git_dir, &id, &bytes)?;

    Ok(id)
}

pub fn write(git_dir: &Path, id: &str, content: &[u8]) -> R<()> {
    write_file(create_file(git_dir, id)?, content)
}

pub fn write_str(git_dir: &Path, id: &str, content: &str) -> R<()> {
    write(git_dir, id, content.as_bytes())
}

fn create_file(git_dir: &Path, id: &str) -> R<File> {
    let (dir, filename) = id.split_at(2);
    let obj_dir = git_dir.join("objects").join(dir);
    fs::create_dir_all(&obj_dir)?;
    let path = obj_dir.join(filename);
    let file = fs::File::create(&path)
        .map_err(|e| format!("Failed to create object {:?}. {}", path, e))?;
    Ok(file)
}

fn open_file(git_dir: &Path, id: &str) -> R<File> {
    let (dir, filename) = id.split_at(2);
    let path = git_dir.join("objects").join(dir).join(filename);
    let file = File::open(&path)
        .map_err(|e| format!("Failed to read object {:?}. {}", path, e))?;
    Ok(file)
}

fn write_file(object: File, content: &[u8]) -> R<()> {
    let mut encoder = ZlibEncoder::new(object, Compression::default());
    encoder.write(content)?;
    Ok(())
}