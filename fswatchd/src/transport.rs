//! Cross-platform IPC transport constants.

#[cfg(unix)]
pub const SOCKET_PATH: &str = "/tmp/fswatchd.sock";

#[cfg(windows)]
pub const PIPE_NAME: &str = r"\\.\pipe\fswatchd";
