//! Cross-platform IPC transport: Unix socket on Unix, Named pipe on Windows.

use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
pub const SOCKET_PATH: &str = "/tmp/fswatchd.sock";

#[cfg(windows)]
pub const PIPE_NAME: &str = r"\\.\pipe\fswatchd";

/// Platform-specific listener that accepts connections.
#[cfg(unix)]
pub struct Listener {
    inner: tokio::net::UnixListener,
}

#[cfg(windows)]
pub struct Listener {
    pipe_name: String,
}

#[cfg(unix)]
impl Listener {
    pub async fn bind() -> std::io::Result<Self> {
        let _ = std::fs::remove_file(SOCKET_PATH);
        let inner = tokio::net::UnixListener::bind(SOCKET_PATH)?;
        eprintln!("Daemon listening on {}", SOCKET_PATH);
        Ok(Self { inner })
    }

    pub async fn accept(&self) -> std::io::Result<Connection> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Connection { inner: stream })
    }
}

#[cfg(windows)]
impl Listener {
    pub async fn bind() -> std::io::Result<Self> {
        eprintln!("Daemon listening on {}", PIPE_NAME);
        Ok(Self {
            pipe_name: PIPE_NAME.to_string(),
        })
    }

    pub async fn accept(&self) -> std::io::Result<Connection> {
        use tokio::net::windows::named_pipe::ServerOptions;

        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&self.pipe_name)?;

        server.connect().await?;
        Ok(Connection { inner: server })
    }
}

/// Platform-specific connection stream.
#[cfg(unix)]
pub struct Connection {
    inner: tokio::net::UnixStream,
}

#[cfg(windows)]
pub struct Connection {
    inner: tokio::net::windows::named_pipe::NamedPipeServer,
}

impl Connection {
    pub fn split(self) -> (impl AsyncRead + Unpin, impl AsyncWrite + Unpin) {
        #[cfg(unix)]
        {
            self.inner.into_split()
        }
        #[cfg(windows)]
        {
            tokio::io::split(self.inner)
        }
    }
}
