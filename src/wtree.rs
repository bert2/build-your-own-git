use std::{fs::{self, DirEntry, File}, io::Read, path::Path};
use crate::{obj::{self, Obj, ObjType}, util, sha::Sha};

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

pub fn checkout(git_dir: &Path, commit: &Sha) -> R<()> {
    fn checkout_tree(git_dir: &Path, parent: &Path, id: &Sha) -> R<()> {
        fs::create_dir_all(parent)?;

        match obj::read(git_dir, id)? {
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
            _ => Err(format!("Object {} is not a tree.", id.value()).into())
        }
    }

    fn checkout_blob(git_dir: &Path, filename: &Path, id: &Sha) -> R<()> {
        match obj::read(git_dir, id)? {
            Obj::Blob { content } => Ok(fs::write(filename, content)?),
            _ => Err(format!("Object {} is not a blob.", id.value()).into())
        }
    }

    let obj = obj::read(git_dir, &commit)?;
    let target_dir = git_dir.parent()
        .ok_or(format!("Reached file system root while trying to get parent of {:?}.", git_dir))?;

    match obj {
        Obj::Commit { tree: t, .. } => {
            clear(git_dir)?;
            checkout_tree(git_dir, target_dir, &t)
        },
        _ => Err(format!("Object {} is not a commit.", commit.value()).into())
    }
}

pub fn write_tree(git_dir: &Path, path: &Path) -> R<Sha> {
    fn render_entry(git_dir: &Path, entry: &DirEntry) -> R<Vec<u8>> {
        fn render_file(entry: &DirEntry) -> R<Vec<u8>> {
            let mode_and_name = format!("100644 {}\x00", util::name(entry));
            let content = read_file(&entry.path())?;
            let id = Sha::generate_raw(&content);
            let mut row = mode_and_name.into_bytes();
            row.extend_from_slice(&id);
            Ok(row)
        }

        fn render_dir(git_dir: &Path, entry: &DirEntry) -> R<Vec<u8>> {
            let mode_and_name = format!("040000 {}\x00", util::name(entry));
            let id = write_tree(git_dir, &entry.path())?;
            let mut row = mode_and_name.into_bytes();
            row.extend_from_slice(&id.to_bytes());
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

    let mut dir_entries = path.read_dir()?.collect::<Result<Vec<_>, _>>()?;
    dir_entries.sort_by_key(util::name);
    let content = dir_entries.iter()
        .filter(|e| util::name(e) != ".git")
        .map(|e| render_entry(git_dir, e))
        .collect::<R<Vec<_>>>()?
        .concat();
    let sha = obj::write(git_dir, ObjType::Tree, &content)?;
    Ok(sha)
}

pub fn read_file(path: &Path) -> R<Vec<u8>> {
    let mut file = File::open(&path)
        .map_err(|e| format!("Failed to open file '{}': {}.", path.to_string_lossy(), e))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read file '{}': {}.", path.to_string_lossy(), e))?;

    Ok(bytes)
}
