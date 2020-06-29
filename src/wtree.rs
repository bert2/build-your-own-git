use std::{fs::{self, DirEntry}, path::Path};
use crate::{obj::{self, Obj}, util, sha};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn clear(git_dir: & Path) -> R<()> {
    let dir_entries = git_dir
        .parent()
        .ok_or(format!("Reached file system root while trying to enumerate parent of {:?}.", git_dir))?
        .read_dir()?
        .collect::<Result<Vec<_>, _>>()?;

    dir_entries.iter()
        .filter(|e| !git_dir.ends_with(e.path()))
        .map(|e| if e.file_type()?.is_file() { fs::remove_file(e.path()) } else { fs::remove_dir_all(e.path()) })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(())
}

pub fn checkout_commit(git_dir: &Path, id: &str) -> R<()> {
    let obj = obj::read_gen(git_dir, &id)?;
    let target_dir = git_dir.parent()
        .ok_or(format!("Reached file system root while trying to get parent of {:?}.", git_dir))?;

    match obj {
        Obj::Commit { tree: t } => {
            clear(git_dir)?;
            checkout_tree(git_dir, target_dir, &t)
        },
        _ => Err(format!("Object {} is not a commit.", id).into())
    }
}

pub fn checkout_tree(git_dir: &Path, parent: &Path, id: &str) -> R<()> {
    fs::create_dir_all(parent)?;

    match obj::read_gen(git_dir, id)? {
        Obj::Tree { entries } => {
            entries.iter()
                .map(|e| match e.mode {
                    040000 => checkout_tree(git_dir, &parent.join(&e.name), &e.id),
                    100644 |
                    100755 => checkout_blob(git_dir, &parent.join(&e.name), &e.id),
                    _      => Err(format!("Tree entry with unsupported file mode: {:#?}", e).into())
                })
                .collect::<R<Vec<_>>>()?;
                Ok(())
        },
        _ => Err(format!("Object {} is not a tree.", id).into())
    }
}

pub fn checkout_blob(git_dir: &Path, filename: &Path, id: &str) -> R<()> {
    match obj::read_gen(git_dir, id)? {
        Obj::Blob { content } => Ok(fs::write(filename, content)?),
        _ => Err(format!("Object {} is not a blob.", id).into())
    }
}

pub fn write(git_dir: &Path, path: &Path) -> R<[u8; 20]> {
    let mut dir_entries = path.read_dir()?.collect::<Result<Vec<_>, _>>()?;
    dir_entries.sort_by_key(util::name);
    let tree_entries = dir_entries.iter()
        .filter(|e| util::name(e) != ".git")
        .map(|e| render_entry(git_dir, e))
        .collect::<R<Vec<_>>>()?
        .concat();
    let mut content = format!("tree {}\x00", tree_entries.len()).into_bytes();
    content.extend(tree_entries);
    let sha = sha::from(&content);
    obj::write(git_dir, &sha::print(&sha), &content)?;
    Ok(sha)
}

fn render_entry(git_dir: &Path, entry: &DirEntry) -> R<Vec<u8>> {
    fn render_file(entry: &DirEntry) -> R<Vec<u8>> {
        let mode_and_name = format!("100644 {}\x00", util::name(entry));
        let content = obj::blob::from_file(&entry.path())?;
        let id = sha::from_str(&content);
        let mut row = mode_and_name.into_bytes();
        row.extend_from_slice(&id);
        Ok(row)
    }

    fn render_dir(git_dir: &Path, entry: &DirEntry) -> R<Vec<u8>> {
        let mode_and_name = format!("040000 {}\x00", util::name(entry));
        let sha = write(git_dir, &entry.path())?;
        let mut row = mode_and_name.into_bytes();
        row.extend_from_slice(&sha);
        Ok(row)
    }

    let _type = entry.file_type()?;
    if _type.is_file() {
        render_file(entry)
    } else if _type.is_dir() {
        render_dir(git_dir, entry)
    } else {
        Err(format!("Symbolic links ('{}') are not supported.", util::name(entry)).into())
    }
}
