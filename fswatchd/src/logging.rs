//! Logging configuration for fswatchd.
//!
//! - Debug builds: console + file output (debug level)
//! - Release builds: file output only (info level)

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const LOG_DIR_NAME: &str = ".fswatchd";
const LOG_FILE_PREFIX: &str = "fswatchd.log";
const LOG_MAX_AGE_SECS: u64 = 24 * 60 * 60; // 1 day

pub fn init() {
    let is_debug = cfg!(debug_assertions);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = if is_debug { "debug" } else { "info" };
        EnvFilter::new(format!("fswatchd={level},warn"))
    });

    let file_appender = setup_file_appender();
    let file_layer = fmt::layer()
        .with_target(false)
        .with_ansi(false)
        .with_writer(file_appender);

    let console_layer = is_debug.then(|| fmt::layer().with_target(false));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();
}

fn setup_file_appender() -> RollingFileAppender {
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(LOG_DIR_NAME)
        .join("logs");

    fs::create_dir_all(&log_dir).ok();
    cleanup_old_logs(&log_dir);

    RollingFileAppender::new(Rotation::DAILY, log_dir, LOG_FILE_PREFIX)
}

fn cleanup_old_logs(log_dir: &PathBuf) {
    let max_age = Duration::from_secs(LOG_MAX_AGE_SECS);
    let Ok(entries) = fs::read_dir(log_dir) else { return };
    let now = SystemTime::now();

    for entry in entries.flatten() {
        let dominated = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);

        if dominated {
            let _ = fs::remove_file(entry.path());
        }
    }
}
