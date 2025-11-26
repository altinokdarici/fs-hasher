use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use xxhash_rust::xxh3::xxh3_64;

#[derive(Error, Debug)]
pub enum HashError {
    #[error("Invalid glob pattern: {0}")]
    InvalidGlob(#[from] ignore::Error),

    #[error("Walk error: {0}")]
    Walk(ignore::Error),

    #[error("Failed to read file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("No files matched the glob pattern")]
    NoFilesMatched,

    #[error("Watch error: {0}")]
    Watch(String),
}

/// Hash a single file
pub fn hash_file(path: &Path) -> Result<u64, std::io::Error> {
    let contents = fs::read(path)?;
    Ok(xxh3_64(&contents))
}

/// Aggregate multiple file hashes into a single hash
pub fn aggregate_hashes(mut hashes: Vec<u64>) -> u64 {
    hashes.sort();
    // Pre-allocate exact size (8 bytes per u64 hash) to avoid reallocations
    let mut bytes = Vec::with_capacity(hashes.len() * 8);
    for hash in &hashes {
        bytes.extend_from_slice(&hash.to_le_bytes());
    }
    xxh3_64(&bytes)
}

/// List files matching a glob pattern in a directory
pub fn list_files(root: &Path, path: &str, glob_pattern: &str) -> Result<Vec<PathBuf>, HashError> {
    let full_path = root.join(path);

    let overrides = OverrideBuilder::new(&full_path)
        .add(glob_pattern)?
        .build()?;

    let walker = WalkBuilder::new(&full_path).overrides(overrides).build();

    let mut files = Vec::new();
    for entry in walker {
        let entry = entry.map_err(HashError::Walk)?;
        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            files.push(entry.path().to_path_buf());
        }
    }

    if files.is_empty() {
        return Err(HashError::NoFilesMatched);
    }

    Ok(files)
}
