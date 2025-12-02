//! NDJSON server over Unix socket / Windows named pipe.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{debug, error, info};

use crate::daemon::{self, DaemonState};
use crate::persistence::{self, PersistedState, WatchEntry};
use crate::protocol::{self, Request, Response, SubscriptionKey};
use crate::session::{RequestResult, Session, SessionBackend};
#[cfg(windows)]
use crate::transport::PIPE_NAME;
#[cfg(unix)]
use crate::transport::SOCKET_PATH;

const FLUSH_INTERVAL_SECS: u64 = 30;
const DEBOUNCE_MS: u64 = 100;

/// Shared application state
struct AppState {
    daemon: RwLock<DaemonState>,
    persisted: RwLock<PersistedState>,
    dirty: AtomicBool,
    event_tx: mpsc::Sender<notify::Event>,
    /// Broadcast channel for file changes - sends (key, paths)
    change_tx: broadcast::Sender<(SubscriptionKey, Vec<String>)>,
    /// Active subscriptions: key -> (root, path, glob)
    subscriptions: RwLock<HashMap<SubscriptionKey, (PathBuf, String, String)>>,
}

/// Backend adapter that connects Session to AppState
struct AppStateBackend {
    state: Arc<AppState>,
}

impl SessionBackend for AppStateBackend {
    fn unwatch(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let key = key.to_string();
        let state = self.state.clone();

        Box::pin(async move {
            // Remove from subscriptions and get the root
            let root = {
                let mut subs = state.subscriptions.write().await;
                subs.remove(&key).map(|(root, _, _)| root)
            };

            let Some(root) = root else {
                return Ok(()); // Already removed or never existed
            };

            // Remove from persisted state
            {
                let mut p = state.persisted.write().await;
                let before = p.watch_entries.len();
                p.watch_entries.retain(|e| {
                    let entry_key = protocol::make_subscription_key(
                        &e.root.to_string_lossy(),
                        &e.path,
                        &e.glob,
                    );
                    entry_key != key
                });
                if p.watch_entries.len() != before {
                    state.dirty.store(true, std::sync::atomic::Ordering::SeqCst);
                    if let Err(e) = persistence::save(&p) {
                        error!("Failed to save state: {}", e);
                    }
                }
            }

            // Check if any other subscriptions still use this root
            let has_other_subscriptions = {
                let subs = state.subscriptions.read().await;
                subs.values().any(|(r, _, _)| r == &root)
            };

            // Also check persisted state
            let has_persisted = {
                let p = state.persisted.read().await;
                p.watch_entries.iter().any(|e| e.root == root)
            };

            // Stop watcher if no more subscriptions for this root
            if !has_other_subscriptions && !has_persisted {
                let mut daemon = state.daemon.write().await;
                daemon::stop_watching(&mut daemon, &root);
            }

            Ok(())
        })
    }

    fn hash(
        &self,
        root: &str,
        path: &str,
        glob: &str,
        persistent: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(String, usize), String>> + Send + '_>,
    > {
        let root = root.to_string();
        let path = path.to_string();
        let glob = glob.to_string();
        let state = self.state.clone();

        Box::pin(async move {
            let root_path = PathBuf::from(&root);

            // If persistent, add to persisted watch entries
            if persistent {
                let entry = WatchEntry {
                    root: root_path.clone(),
                    path: path.clone(),
                    glob: glob.clone(),
                };
                let mut p = state.persisted.write().await;
                if p.watch_entries.insert(entry) {
                    state.dirty.store(true, Ordering::SeqCst);
                    if let Err(e) = persistence::save(&p) {
                        error!("Failed to save state: {}", e);
                    }
                }
            }

            let mut daemon = state.daemon.write().await;
            match daemon::hash(
                &mut daemon,
                &root_path,
                &path,
                &glob,
                persistent,
                Some(state.event_tx.clone()),
            ) {
                Ok(result) => Ok((format!("{:016x}", result.hash), result.file_count)),
                Err(e) => Err(e.to_string()),
            }
        })
    }

    fn watch(
        &self,
        root: &str,
        path: &str,
        glob: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
        let root = root.to_string();
        let path = path.to_string();
        let glob = glob.to_string();
        let state = self.state.clone();

        Box::pin(async move {
            let root_path = PathBuf::from(&root);

            // Start watching if not already
            {
                let mut daemon = state.daemon.write().await;
                if let Err(e) =
                    daemon::ensure_watching(&mut daemon, &root_path, Some(state.event_tx.clone()))
                {
                    return Err(e.to_string());
                }
            }

            // Add to persisted watch entries
            {
                let entry = WatchEntry {
                    root: root_path.clone(),
                    path: path.clone(),
                    glob: glob.clone(),
                };
                let mut p = state.persisted.write().await;
                if p.watch_entries.insert(entry) {
                    state.dirty.store(true, Ordering::SeqCst);
                    if let Err(e) = persistence::save(&p) {
                        error!("Failed to save state: {}", e);
                    }
                }
            }

            Ok(())
        })
    }
}

#[tokio::main]
pub async fn run(socket_path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    let socket_path = socket_path.unwrap_or_else(|| SOCKET_PATH.to_string());
    #[cfg(windows)]
    let socket_path = socket_path.unwrap_or_else(|| PIPE_NAME.to_string());

    // Check if another daemon is already running
    #[cfg(unix)]
    {
        if tokio::net::UnixStream::connect(&socket_path).await.is_ok() {
            return Err("Another fswatchd instance is already running".into());
        }
        let _ = std::fs::remove_file(&socket_path);
    }

    let (event_tx, mut event_rx) = mpsc::channel::<notify::Event>(100);
    let (change_tx, _) = broadcast::channel::<(SubscriptionKey, Vec<String>)>(100);

    let state = Arc::new(AppState {
        daemon: RwLock::new(DaemonState::new()),
        persisted: RwLock::new(persistence::load()),
        dirty: AtomicBool::new(false),
        event_tx,
        change_tx: change_tx.clone(),
        subscriptions: RwLock::new(HashMap::new()),
    });

    // Restore watchers from persisted state
    restore_watchers(&state).await;

    // Handle file change events from notify
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut pending: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();
        let mut interval = tokio::time::interval(Duration::from_millis(DEBOUNCE_MS));

        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                            let deadline = tokio::time::Instant::now() + Duration::from_millis(DEBOUNCE_MS);
                            for path in event.paths {
                                pending.insert(path, deadline);
                            }
                        }
                        _ => {}
                    }
                }
                _ = interval.tick() => {
                    let now = tokio::time::Instant::now();
                    let ready: Vec<PathBuf> = pending
                        .iter()
                        .filter(|(_, deadline)| now >= **deadline)
                        .map(|(path, _)| path.clone())
                        .collect();

                    if !ready.is_empty() {
                        for path in &ready {
                            pending.remove(path);
                            // Invalidate cache
                            let mut daemon = state_clone.daemon.write().await;
                            daemon::invalidate_file(&mut daemon, path);
                        }

                        // Check which subscriptions match and notify
                        let subs = state_clone.subscriptions.read().await;
                        let mut matches: HashMap<SubscriptionKey, Vec<String>> = HashMap::new();

                        for (key, (root, path, glob)) in subs.iter() {
                            for changed_path in &ready {
                                if matches_watch(changed_path, root, path, glob) {
                                    matches
                                        .entry(key.clone())
                                        .or_default()
                                        .push(changed_path.to_string_lossy().to_string());
                                }
                            }
                        }

                        for (key, paths) in matches {
                            let _ = state_clone.change_tx.send((key, paths));
                        }
                    }
                }
            }
        }
    });

    // Periodic flush (30s if dirty)
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
        loop {
            interval.tick().await;
            if state_clone.dirty.swap(false, Ordering::SeqCst) {
                let persisted = state_clone.persisted.read().await;
                if let Err(e) = persistence::save(&persisted) {
                    error!("Failed to save state: {}", e);
                }
            }
        }
    });

    // Start accepting connections
    accept_connections(state, &socket_path).await
}

#[cfg(unix)]
async fn accept_connections(
    state: Arc<AppState>,
    socket_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::UnixListener::bind(socket_path)?;
    info!("Daemon started, listening on {}", socket_path);

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                debug!("Connection closed: {}", e);
            }
        });
    }
}

#[cfg(windows)]
async fn accept_connections(
    state: Arc<AppState>,
    pipe_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::windows::named_pipe::ServerOptions;

    info!("Daemon started, listening on {}", pipe_name);

    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name)?;

    loop {
        server.connect().await?;
        let stream = server;

        // Create next pipe instance before serving
        server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(pipe_name)?;

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(state, stream).await {
                debug!("Connection closed: {}", e);
            }
        });
    }
}

/// Handle a single client connection
async fn handle_connection<S>(
    state: Arc<AppState>,
    stream: S,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Subscribe to change events for this connection
    let mut change_rx = state.change_tx.subscribe();

    // Create session for this connection
    let mut session = Session::new();

    // Create backend adapter
    let backend = AppStateBackend {
        state: state.clone(),
    };

    loop {
        line.clear();

        // Wait for either a request or timeout (to check for events)
        let read_result =
            tokio::time::timeout(Duration::from_millis(50), reader.read_line(&mut line)).await;

        match read_result {
            Ok(Ok(0)) => break, // Connection closed
            Ok(Ok(_)) => {
                // Got a request - parse and process
                let response = match serde_json::from_str::<Request>(&line) {
                    Ok(req) => {
                        let result = session.process_request(req, &backend).await;
                        match result {
                            RequestResult::Response(resp) => resp,
                            RequestResult::Subscribe { response, key } => {
                                // Add to global subscriptions
                                register_subscription(&state, &key, &line).await;
                                response
                            }
                            RequestResult::Unsubscribe { response } => response,
                        }
                    }
                    Err(e) => Response::Error {
                        error: format!("Invalid request: {}", e),
                    },
                };

                let response_json = serde_json::to_string(&response)?;
                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            Ok(Err(e)) => return Err(e.into()), // Read error
            Err(_) => {}                        // Timeout - no request, check for events below
        }

        // Drain any pending events (non-blocking)
        loop {
            match change_rx.try_recv() {
                Ok((key, paths)) => {
                    if session.should_receive_event(&key) {
                        let event = protocol::SubscriptionEvent { key, paths };
                        let event_json = serde_json::to_string(&event)?;
                        writer.write_all(event_json.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue, // Skip missed events
                Err(broadcast::error::TryRecvError::Closed) => return Ok(()),
            }
        }
        writer.flush().await?;
    }

    Ok(())
}

/// Register a subscription in the global state
async fn register_subscription(state: &Arc<AppState>, key: &str, request_line: &str) {
    // Parse the request again to get root/path/glob
    if let Ok(Request::Watch { root, path, glob }) = serde_json::from_str(request_line) {
        let root_path = PathBuf::from(&root);
        let mut subs = state.subscriptions.write().await;
        subs.insert(key.to_string(), (root_path, path, glob));
    }
}

/// Restore watchers from persisted state
async fn restore_watchers(state: &Arc<AppState>) {
    let entries: Vec<WatchEntry> = {
        let p = state.persisted.read().await;
        p.watch_entries.iter().cloned().collect()
    };

    if entries.is_empty() {
        return;
    }

    info!(
        "Restoring {} watch entries from persisted state",
        entries.len()
    );

    for entry in entries {
        {
            let mut daemon = state.daemon.write().await;
            if let Err(e) =
                daemon::ensure_watching(&mut daemon, &entry.root, Some(state.event_tx.clone()))
            {
                error!(
                    "Failed to restore watcher for {}: {}",
                    entry.root.display(),
                    e
                );
                continue;
            }
        }

        // Register subscription
        let key = protocol::make_subscription_key(
            &entry.root.to_string_lossy(),
            &entry.path,
            &entry.glob,
        );
        {
            let mut subs = state.subscriptions.write().await;
            subs.insert(
                key,
                (entry.root.clone(), entry.path.clone(), entry.glob.clone()),
            );
        }

        // Background re-hash
        let state_clone = state.clone();
        tokio::spawn(async move {
            debug!(
                "Background re-hash for: {} path={} glob={}",
                entry.root.display(),
                entry.path,
                entry.glob
            );
            let start = std::time::Instant::now();
            let mut daemon = state_clone.daemon.write().await;
            match daemon::hash(
                &mut daemon,
                &entry.root,
                &entry.path,
                &entry.glob,
                false,
                None,
            ) {
                Ok(result) => {
                    info!(
                        "Re-hash complete: {} path={} files={} duration={:?}",
                        entry.root.display(),
                        entry.path,
                        result.file_count,
                        start.elapsed()
                    );
                }
                Err(e) => {
                    error!(
                        "Background re-hash failed for {} path={}: {}",
                        entry.root.display(),
                        entry.path,
                        e
                    );
                }
            }
        });
    }
}

/// Check if a changed file path matches a watch subscription
fn matches_watch(
    changed: &std::path::Path,
    root: &std::path::Path,
    path: &str,
    glob_pattern: &str,
) -> bool {
    let watch_dir = match root.join(path).canonicalize() {
        Ok(p) => p,
        Err(_) => root.join(path),
    };
    let changed = match changed.canonicalize() {
        Ok(p) => p,
        Err(_) => changed.to_path_buf(),
    };

    if !changed.starts_with(&watch_dir) {
        return false;
    }

    let rel_path = match changed.strip_prefix(&watch_dir) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let glob = match globset::Glob::new(glob_pattern) {
        Ok(g) => g.compile_matcher(),
        Err(_) => return false,
    };

    glob.is_match(rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_watch_basic() {
        let temp_dir = std::env::temp_dir().join("fswatchd-test-matches");
        let _ = std::fs::create_dir_all(&temp_dir);
        let test_file = temp_dir.join("test.rs");
        let _ = std::fs::write(&test_file, "");

        assert!(matches_watch(&test_file, &temp_dir, ".", "*.rs"));
        assert!(!matches_watch(&test_file, &temp_dir, ".", "*.txt"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_matches_watch_nested() {
        let temp_dir = std::env::temp_dir().join("fswatchd-test-nested");
        let sub_dir = temp_dir.join("src");
        let _ = std::fs::create_dir_all(&sub_dir);
        let test_file = sub_dir.join("lib.rs");
        let _ = std::fs::write(&test_file, "");

        assert!(matches_watch(&test_file, &temp_dir, ".", "**/*.rs"));
        assert!(matches_watch(&test_file, &temp_dir, "src", "*.rs"));
        assert!(!matches_watch(&test_file, &temp_dir, "lib", "*.rs"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
