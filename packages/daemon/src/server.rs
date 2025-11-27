//! IPC server: handles JSON protocol over Unix socket (Unix) or Named pipe (Windows).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::daemon::{self, DaemonState};
use crate::persistence::{self, PersistedState};
use crate::transport::Listener;

const FLUSH_INTERVAL_SECS: u64 = 30;

type SharedState = Arc<RwLock<DaemonState>>;
type SharedPersistence = Arc<RwLock<PersistedState>>;

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "hash")]
    Hash {
        root: String,
        path: String,
        glob: String,
        #[serde(default)]
        persistent: bool,
    },
    #[serde(rename = "watch")]
    Watch {
        root: String,
        path: String,
        glob: String,
    },
}

#[derive(serde::Serialize)]
struct HashResponse {
    hash: String,
    file_count: usize,
}

#[derive(serde::Serialize)]
struct WatchEvent {
    r#type: &'static str,
}

#[derive(serde::Serialize)]
struct ErrorResponse {
    error: String,
}

/// File change event broadcast to all connections.
#[derive(Clone, Debug)]
struct FileChange {
    path: PathBuf,
}

#[tokio::main]
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let listener = Listener::bind().await?;

    let state: SharedState = Arc::new(RwLock::new(DaemonState::new()));
    let persisted: SharedPersistence = Arc::new(RwLock::new(persistence::load()));
    let dirty = Arc::new(AtomicBool::new(false));

    let (event_tx, mut event_rx) = mpsc::channel::<notify::Event>(100);
    let (change_tx, _) = broadcast::channel::<FileChange>(100);

    // Restore watchers from persisted state and trigger background re-hash
    restore_watchers(&state, &persisted, &event_tx).await;

    // Handle file change events from notify
    let state_clone = state.clone();
    let change_tx_clone = change_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            handle_file_event(&state_clone, &change_tx_clone, event).await;
        }
    });

    // Periodic flush (30s if dirty)
    let persisted_clone = persisted.clone();
    let dirty_clone = dirty.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if dirty_clone.swap(false, Ordering::SeqCst) {
                let state = persisted_clone.read().await;
                if let Err(e) = persistence::save(&state) {
                    eprintln!("Failed to save state: {}", e);
                }
            }
        }
    });

    loop {
        let conn = listener.accept().await?;
        let state = state.clone();
        let persisted = persisted.clone();
        let dirty = dirty.clone();
        let event_tx = event_tx.clone();
        let change_rx = change_tx.subscribe();

        tokio::spawn(async move {
            if let Err(e) =
                handle_connection(conn, state, persisted, dirty, event_tx, change_rx).await
            {
                eprintln!("Connection error: {}", e);
            }
        });
    }
}

/// Restore watchers from persisted state and trigger background re-hash.
async fn restore_watchers(
    state: &SharedState,
    persisted: &SharedPersistence,
    event_tx: &mpsc::Sender<notify::Event>,
) {
    let roots: Vec<PathBuf> = {
        let p = persisted.read().await;
        p.watch_roots.iter().cloned().collect()
    };

    if roots.is_empty() {
        return;
    }

    eprintln!("Restoring {} watch roots from persisted state", roots.len());

    for root in roots {
        // Start watcher
        {
            let mut s = state.write().await;
            if let Err(e) = daemon::ensure_watching(&mut s, &root, Some(event_tx.clone())) {
                eprintln!("Failed to restore watcher for {}: {}", root.display(), e);
                continue;
            }
        }

        // Background re-hash to populate cache
        let state_clone = state.clone();
        let root_clone = root.clone();
        tokio::spawn(async move {
            eprintln!("Background re-hash for: {}", root_clone.display());
            let mut s = state_clone.write().await;
            // Hash all files under root with "**/*" glob to populate cache
            if let Err(e) = daemon::hash(&mut s, &root_clone, ".", "**/*", false, None) {
                eprintln!(
                    "Background re-hash failed for {}: {}",
                    root_clone.display(),
                    e
                );
            } else {
                eprintln!("Background re-hash complete for: {}", root_clone.display());
            }
        });
    }
}

async fn handle_file_event(
    state: &SharedState,
    change_tx: &broadcast::Sender<FileChange>,
    event: notify::Event,
) {
    use notify::EventKind;

    match event.kind {
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
            for path in event.paths {
                eprintln!("File changed: {}", path.display());
                let mut state = state.write().await;
                daemon::invalidate_file(&mut state, &path);
                let _ = change_tx.send(FileChange { path });
            }
        }
        _ => {}
    }
}

async fn handle_connection(
    conn: crate::transport::Connection,
    state: SharedState,
    persisted: SharedPersistence,
    dirty: Arc<AtomicBool>,
    event_tx: mpsc::Sender<notify::Event>,
    mut change_rx: broadcast::Receiver<FileChange>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = conn.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Track active watch subscriptions for this connection
    let mut watches: Vec<(PathBuf, String, String)> = Vec::new(); // (root, path, glob)

    loop {
        tokio::select! {
            // Handle incoming requests
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => break, // Connection closed
                    Ok(_) => {
                        let response = process_request(&line, &state, &persisted, &dirty, &event_tx, &mut watches).await;
                        if let Some(resp) = response {
                            writer.write_all(resp.as_bytes()).await?;
                            writer.write_all(b"\n").await?;
                        }
                        line.clear();
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            // Handle file change broadcasts
            result = change_rx.recv() => {
                if let Ok(change) = result {
                    // Check if change matches any watch subscription
                    for (root, path, glob) in &watches {
                        if matches_watch(&change.path, root, path, glob) {
                            let event = serde_json::to_string(&WatchEvent { r#type: "changed" }).unwrap();
                            writer.write_all(event.as_bytes()).await?;
                            writer.write_all(b"\n").await?;
                            break; // One notification per change event
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Check if a changed file path matches a watch subscription.
fn matches_watch(changed: &Path, root: &Path, path: &str, glob: &str) -> bool {
    use ignore::overrides::OverrideBuilder;

    let watch_dir = root.join(path);

    // Check if the changed file is under the watched directory
    if !changed.starts_with(&watch_dir) {
        return false;
    }

    // Check glob pattern using ignore crate
    let overrides = match OverrideBuilder::new(&watch_dir)
        .add(glob)
        .and_then(|b| b.build())
    {
        Ok(o) => o,
        Err(_) => return false,
    };

    overrides.matched(changed, false).is_whitelist()
}

async fn process_request(
    request: &str,
    state: &SharedState,
    persisted: &SharedPersistence,
    dirty: &Arc<AtomicBool>,
    event_tx: &mpsc::Sender<notify::Event>,
    watches: &mut Vec<(PathBuf, String, String)>,
) -> Option<String> {
    match serde_json::from_str::<Request>(request) {
        Ok(Request::Hash {
            root,
            path,
            glob,
            persistent,
        }) => {
            let root_path = PathBuf::from(&root);
            let mut state = state.write().await;

            // If persistent, add to persisted watch roots
            if persistent {
                let mut p = persisted.write().await;
                if p.watch_roots.insert(root_path.clone()) {
                    dirty.store(true, Ordering::SeqCst);
                    // Save immediately on new watch entry
                    if let Err(e) = persistence::save(&p) {
                        eprintln!("Failed to save state: {}", e);
                    }
                }
            }

            let response = match daemon::hash(
                &mut state,
                &root_path,
                &path,
                &glob,
                persistent,
                Some(event_tx.clone()),
            ) {
                Ok(result) => serde_json::to_string(&HashResponse {
                    hash: format!("{:016x}", result.hash),
                    file_count: result.file_count,
                })
                .unwrap(),
                Err(e) => serde_json::to_string(&ErrorResponse {
                    error: e.to_string(),
                })
                .unwrap(),
            };
            Some(response)
        }
        Ok(Request::Watch { root, path, glob }) => {
            let root_path = PathBuf::from(&root);

            // Start watching if not already
            {
                let mut state = state.write().await;
                if let Err(e) =
                    daemon::ensure_watching(&mut state, &root_path, Some(event_tx.clone()))
                {
                    return Some(
                        serde_json::to_string(&ErrorResponse {
                            error: e.to_string(),
                        })
                        .unwrap(),
                    );
                }
            }

            // Add to persisted watch roots
            {
                let mut p = persisted.write().await;
                if p.watch_roots.insert(root_path.clone()) {
                    dirty.store(true, Ordering::SeqCst);
                    if let Err(e) = persistence::save(&p) {
                        eprintln!("Failed to save state: {}", e);
                    }
                }
            }

            // Add to this connection's watch list
            watches.push((root_path, path, glob));

            // No response for watch - just start streaming
            None
        }
        Err(e) => Some(
            serde_json::to_string(&ErrorResponse {
                error: format!("Invalid request: {}", e),
            })
            .unwrap(),
        ),
    }
}
