//! Daemon-specific logic: watcher management and cache invalidation.

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::hash_service::{self, HashResult};
use crate::hasher;

/// Cache key for glob hash results
#[derive(Hash, Eq, PartialEq, Clone)]
pub struct GlobKey {
    pub root: PathBuf,
    pub path: String,
    pub glob: String,
}

/// Daemon state: file cache + result cache + active watchers.
pub struct DaemonState {
    pub file_cache: HashMap<PathBuf, u64>,
    pub result_cache: HashMap<GlobKey, HashResult>,
    pub root_watchers: HashMap<PathBuf, RecommendedWatcher>,
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            file_cache: HashMap::new(),
            result_cache: HashMap::new(),
            root_watchers: HashMap::new(),
        }
    }
}

/// Invalidates cached hash for a file path.
pub fn invalidate_file(state: &mut DaemonState, path: &PathBuf) {
    if state.file_cache.remove(path).is_some() {
        debug!(path = %path.display(), "invalidated file cache");
    }

    // Invalidate any result cache entries that could contain this file
    let keys_to_remove: Vec<GlobKey> = state
        .result_cache
        .keys()
        .filter(|key| path.starts_with(key.root.join(&key.path)))
        .cloned()
        .collect();

    for key in keys_to_remove {
        state.result_cache.remove(&key);
        debug!(path = %key.path, glob = %key.glob, "invalidated result cache");
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

    // Check result cache first
    let key = GlobKey {
        root: root.clone(),
        path: path.to_string(),
        glob: glob.to_string(),
    };

    if let Some(result) = state.result_cache.get(&key) {
        debug!(path = %path, glob = %glob, "cache hit");
        return Ok(*result);
    }

    // Cache miss - compute and store
    let result = hash_service::hash_with_cache(&mut state.file_cache, root, path, glob)?;
    state.result_cache.insert(key, result);
    Ok(result)
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
            // Use try_send to avoid blocking - if channel is full, event is dropped
            // This is safer than blocking_send which can have issues from non-tokio threads
            let _ = tx.try_send(event);
        }
    })
    .map_err(|e| hasher::HashError::Watch(e.to_string()))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| hasher::HashError::Watch(e.to_string()))?;

    info!(root = %root.display(), "started watching");
    state.root_watchers.insert(root.clone(), watcher);

    Ok(())
}

/// Stops watching a root directory if it exists.
pub fn stop_watching(state: &mut DaemonState, root: &PathBuf) -> bool {
    if state.root_watchers.remove(root).is_some() {
        info!(root = %root.display(), "stopped watching");
        true
    } else {
        false
    }
}
