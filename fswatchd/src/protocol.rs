//! NDJSON protocol types and parsing.
//!
//! This module contains all protocol-related types and logic, making it easy to test
//! serialization/deserialization without needing actual socket connections.

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_128;

/// Subscription key type (128-bit xxh3 hash as 32-char hex string)
pub type SubscriptionKey = String;

/// Request types from client
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum Request {
    Hash {
        root: String,
        path: String,
        glob: String,
        #[serde(default)]
        persistent: bool,
    },
    Watch {
        root: String,
        path: String,
        glob: String,
    },
    Unwatch {
        key: String,
    },
}

/// Response types to client
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Response {
    Hash { hash: String, file_count: usize },
    Watch { key: String },
    Ok { ok: bool },
    Error { error: String },
}

/// Subscription event pushed to client
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    pub key: String,
    pub paths: Vec<String>,
}

/// Generate deterministic 128-bit subscription key from root/path/glob.
pub fn make_subscription_key(root: &str, path: &str, glob: &str) -> SubscriptionKey {
    let input = format!("{}\0{}\0{}", root, path, glob);
    let hash = xxh3_128(input.as_bytes());
    format!("{:032x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_key_deterministic() {
        let key1 = make_subscription_key("/repo", "src", "**/*.rs");
        let key2 = make_subscription_key("/repo", "src", "**/*.rs");
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 32); // 128-bit hex
    }

    #[test]
    fn test_subscription_key_unique() {
        let key1 = make_subscription_key("/repo", "src", "**/*.rs");
        let key2 = make_subscription_key("/repo", "lib", "**/*.rs");
        let key3 = make_subscription_key("/other", "src", "**/*.rs");
        assert_ne!(key1, key2);
        assert_ne!(key1, key3);
    }
}
