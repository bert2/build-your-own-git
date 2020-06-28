use std::{fs, path::{Path, PathBuf}};

type R<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn init(path: &Path) -> R<PathBuf> {
    let git_dir = path.join(".git");
    fs::create_dir_all(git_dir.join("objects/info"))?;
    fs::create_dir_all(git_dir.join("objects/pack"))?;
    fs::create_dir_all(git_dir.join("refs/heads"))?;
    fs::create_dir_all(git_dir.join("refs/tags"))?;
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/master\n")?;
    Ok(git_dir)
}
