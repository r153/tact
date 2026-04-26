use std::{
    cmp::Ordering,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct FsEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn read_directory(path: &Path) -> io::Result<Vec<FsEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let file_type = metadata.file_type();
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push(FsEntry {
            name,
            path,
            is_dir: file_type.is_dir(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

pub fn entry_from_path(path: PathBuf) -> io::Result<FsEntry> {
    let metadata = fs::metadata(&path)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Ok(FsEntry {
        name,
        path,
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

pub fn sort_entries(entries: &mut [FsEntry]) {
    entries.sort_by(compare_entries);
}

fn compare_entries(a: &FsEntry, b: &FsEntry) -> Ordering {
    match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
}
