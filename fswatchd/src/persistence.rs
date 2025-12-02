//! Persistence layer: save/load watch entries to disk.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

const STATE_DIR: &str = ".fswatchd";
const STATE_FILE: &str = "state.json";

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct WatchEntry {
    pub root: PathBuf,
    pub path: String,
    pub glob: String,
}

impl Hash for WatchEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.root.hash(state);
        self.path.hash(state);
        self.glob.hash(state);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct PersistedState {
    pub watch_entries: HashSet<WatchEntry>,
}

/// Returns the path to the state file (~/.fs-hasher/state.json).
fn state_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(STATE_DIR).join(STATE_FILE))
}

/// Load persisted state from disk.
pub fn load() -> PersistedState {
    let Some(path) = state_file_path() else {
        return PersistedState::default();
    };

    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => PersistedState::default(),
    }
}

/// Save state to disk.
pub fn save(state: &PersistedState) -> Result<(), std::io::Error> {
    let Some(path) = state_file_path() else {
        return Ok(());
    };

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(state)?;
    fs::write(&path, contents)?;

    eprintln!("Saved state to: {}", path.display());
    Ok(())
}
