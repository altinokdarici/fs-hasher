//! Orchestrates file hashing with caching. Reusable across daemon, CLI, or other contexts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::hasher;

pub struct HashResult {
    pub hash: u64,
    pub file_count: usize,
}

/// Hashes files matching a glob pattern, using cache for previously hashed files.
pub fn hash_with_cache(
    cache: &mut HashMap<PathBuf, u64>,
    root: &Path,
    path: &str,
    glob: &str,
) -> Result<HashResult, hasher::HashError> {
    let files = hasher::list_files(root, path, glob)?;
    let file_count = files.len();

    let mut hashes = Vec::with_capacity(file_count);
    for file in files {
        let hash = if let Some(&cached) = cache.get(&file) {
            cached
        } else {
            let h = hasher::hash_file(&file).map_err(|e| hasher::HashError::ReadFile {
                path: file.clone(),
                source: e,
            })?;
            cache.insert(file, h);
            h
        };
        hashes.push(hash);
    }

    let hash = hasher::aggregate_hashes(hashes);
    Ok(HashResult { hash, file_count })
}
