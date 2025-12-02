//! Session handling for client connections.
//!
//! This module contains the per-connection session logic, separated from
//! the actual I/O to enable unit testing.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;

use crate::protocol::{self, Request, Response, SubscriptionKey};

/// Result of processing a request
#[derive(Debug)]
pub enum RequestResult {
    /// Send response to client
    Response(Response),
    /// Send response and add subscription
    Subscribe { response: Response, key: SubscriptionKey },
    /// Send response and remove subscription
    Unsubscribe { response: Response },
}

/// Trait for the backend that handles actual hash/watch operations.
/// This allows mocking in tests. Uses async methods for real implementation.
pub trait SessionBackend: Send + Sync {
    fn hash(
        &self,
        root: &str,
        path: &str,
        glob: &str,
        persistent: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(String, usize), String>> + Send + '_>>;

    fn watch(
        &self,
        root: &str,
        path: &str,
        glob: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;
}

/// Per-connection session state
pub struct Session {
    subscriptions: HashSet<SubscriptionKey>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            subscriptions: HashSet::new(),
        }
    }

    /// Check if this session should receive an event for the given key
    pub fn should_receive_event(&self, key: &SubscriptionKey) -> bool {
        self.subscriptions.contains(key)
    }

    /// Process a request and return the result
    pub async fn process_request<B: SessionBackend>(
        &mut self,
        request: Request,
        backend: &B,
    ) -> RequestResult {
        match request {
            Request::Hash { root, path, glob, persistent } => {
                match backend.hash(&root, &path, &glob, persistent).await {
                    Ok((hash, file_count)) => {
                        RequestResult::Response(Response::Hash { hash, file_count })
                    }
                    Err(e) => RequestResult::Response(Response::Error { error: e }),
                }
            }

            Request::Watch { root, path, glob } => {
                let key = protocol::make_subscription_key(&root, &path, &glob);

                if let Err(e) = backend.watch(&root, &path, &glob).await {
                    return RequestResult::Response(Response::Error {
                        error: format!("Failed to start watcher: {}", e),
                    });
                }

                self.subscriptions.insert(key.clone());

                RequestResult::Subscribe {
                    response: Response::Watch { key: key.clone() },
                    key,
                }
            }

            Request::Unwatch { key } => {
                self.subscriptions.remove(&key);
                RequestResult::Unsubscribe {
                    response: Response::Ok { ok: true },
                }
            }
        }
    }

}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend;

    impl SessionBackend for MockBackend {
        fn hash(
            &self,
            _root: &str,
            _path: &str,
            _glob: &str,
            _persistent: bool,
        ) -> Pin<Box<dyn Future<Output = Result<(String, usize), String>> + Send + '_>> {
            Box::pin(async { Ok(("abc123".to_string(), 5)) })
        }

        fn watch(
            &self,
            _root: &str,
            _path: &str,
            _glob: &str,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_watch_adds_subscription() {
        let mut session = Session::new();
        let backend = MockBackend;

        let request = Request::Watch {
            root: "/repo".to_string(),
            path: "src".to_string(),
            glob: "**/*.rs".to_string(),
        };

        let result = session.process_request(request, &backend).await;

        match result {
            RequestResult::Subscribe { key, .. } => {
                assert!(session.should_receive_event(&key));
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[tokio::test]
    async fn test_unwatch_removes_subscription() {
        let mut session = Session::new();
        let backend = MockBackend;

        // First subscribe
        let request: Request = Request::Watch {
            root: "/repo".to_string(),
            path: "src".to_string(),
            glob: "**/*.rs".to_string(),
        };
        let key = match session.process_request(request, &backend).await {
            RequestResult::Subscribe { key, .. } => key,
            _ => panic!("Expected Subscribe"),
        };

        assert!(session.should_receive_event(&key));

        // Now unsubscribe
        let request = Request::Unwatch { key: key.clone() };
        session.process_request(request, &backend).await;

        assert!(!session.should_receive_event(&key));
    }

    #[tokio::test]
    async fn test_should_receive_event_only_for_subscribed_keys() {
        let mut session = Session::new();
        let backend = MockBackend;

        let request = Request::Watch {
            root: "/repo".to_string(),
            path: "src".to_string(),
            glob: "**/*.rs".to_string(),
        };
        let key = match session.process_request(request, &backend).await {
            RequestResult::Subscribe { key, .. } => key,
            _ => panic!("Expected Subscribe"),
        };

        assert!(session.should_receive_event(&key));
        assert!(!session.should_receive_event(&"other-key".to_string()));
    }
}
