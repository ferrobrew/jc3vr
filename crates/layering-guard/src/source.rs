//! Reading a crate's Rust sources for the checks that look inside them.

use std::{fs, io, path::Path};

/// A source file read from the tree.
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// The path relative to the repository root, with `/` separators.
    pub path: String,
    pub text: String,
}

/// Reads every `.rs` file under `repo/root`, in a stable order.
pub fn read_sources(repo: &Path, root: &str) -> io::Result<Vec<SourceFile>> {
    let mut files = Vec::new();
    collect(&repo.join(root), root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn collect(dir: &Path, rel: &str, out: &mut Vec<SourceFile>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{rel}/{name}");
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), &path, out)?;
        } else if name.ends_with(".rs") {
            let text = fs::read_to_string(entry.path())?;
            out.push(SourceFile { path, text });
        }
    }
    Ok(())
}
