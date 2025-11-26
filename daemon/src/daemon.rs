//! Daemon-specific logic: watcher management and cache invalidation.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::hasher;
use crate::hash_service::{self, HashResult};

/// Daemon state: file cache + active watchers.
pub struct DaemonState {
    pub file_cache: HashMap<PathBuf, u64>,
    pub root_watchers: HashMap<PathBuf, RecommendedWatcher>,
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
            root_watchers: HashMap::new(),
        }
    }
}

/// Invalidates cached hash for a file path.
pub fn invalidate_file(state: &mut DaemonState, path: &PathBuf) {
    if state.file_cache.remove(path).is_some() {
        eprintln!("Invalidated cache for: {}", path.display());
    }
}

/// Hashes files, optionally starting a watcher for the root directory.
pub fn hash(
    state: &mut DaemonState,
    root: &PathBuf,
    path: &str,
    glob: &str,
    persistent: bool,
    event_tx: Option<mpsc::Sender<Event>>,
) -> Result<HashResult, hasher::HashError> {
    if persistent {
        start_watching(state, root, event_tx)?;
    }
    hash_service::hash_with_cache(&mut state.file_cache, root, path, glob)
}

/// Ensures a watcher is running on a root directory. Public for watch API.
pub fn ensure_watching(
    state: &mut DaemonState,
    root: &PathBuf,
    event_tx: Option<mpsc::Sender<Event>>,
) -> Result<(), hasher::HashError> {
    start_watching(state, root, event_tx)
}

/// Starts a recursive watcher on a root directory if not already watching.
fn start_watching(
    state: &mut DaemonState,
    root: &PathBuf,
    event_tx: Option<mpsc::Sender<Event>>,
) -> Result<(), hasher::HashError> {
    if state.root_watchers.contains_key(root) {
        return Ok(());
    }

    let tx = match event_tx {
        Some(tx) => tx,
        None => return Ok(()),
    };

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.blocking_send(event);
        }
    })
    .map_err(|e| hasher::HashError::Watch(e.to_string()))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| hasher::HashError::Watch(e.to_string()))?;

    eprintln!("Started watching: {}", root.display());
    state.root_watchers.insert(root.clone(), watcher);

    Ok(())
}
