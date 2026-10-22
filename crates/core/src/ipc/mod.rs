//! Local IPC: a Unix socket on macOS and Linux, a named pipe on Windows
//!
//! The lease door, the bridges and the forwarders each listen at a socket path.
//! This is the one place that path becomes a transport. The async half is
//! shep's own (`shep_core::transport`), which also carries the shepherd's
//! control socket, so kelpie's door and shep's speak the same way on each
//! platform. [`blocking`] is the same for the threads that serve a bridge or a
//! forwarder.
//!
//! A socket path keeps naming the endpoint everywhere. On Windows [`address`]
//! turns it into a pipe's name, so a path under one home never names the pipe
//! another home's kelpie opened.

use std::io;
use std::path::{Path, PathBuf};

use shep_client::shep_core::transport;

pub use transport::{
    ClientStream, ServerReadHalf, ServerStream, ServerWriteHalf, connected_pair, split,
};

pub mod blocking;
#[cfg(test)]
mod tests;

/// The longest path a Unix socket may have, one byte short of macOS's 104
pub const LONGEST_SOCKET: usize = 103;

/// Whether `socket` is too long to bind, which only a Unix socket's path can be
#[must_use]
pub fn too_long(socket: &Path) -> bool {
    cfg!(unix) && socket.as_os_str().len() > LONGEST_SOCKET
}

/// Where the transport listens for the endpoint `socket` names
///
/// The path itself on Unix. On Windows `\\.\pipe\kelpie-<name>-<digest>`, the
/// digest taken over the whole path, so two homes get two pipes. The result is
/// only path-shaped: never take its parent or ask whether it exists.
#[must_use]
pub fn address(socket: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        socket.to_owned()
    }
    #[cfg(windows)]
    {
        PathBuf::from(pipe_name(socket))
    }
}

// Windows reads a path without regard to case or to which slash divides it, so
// every spelling of one path must name one pipe, or two dogs could both open it.
#[cfg(any(windows, test))]
fn pipe_name(socket: &Path) -> String {
    use sha1::{Digest, Sha1};
    // Bounds the readable half: a pipe's name may be 256 characters.
    const MAX_STEM: usize = 48;

    let whole = socket.to_string_lossy().to_lowercase().replace('/', r"\");
    let digest = Sha1::digest(whole.as_bytes());
    let digest: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let file = whole.rsplit('\\').next().unwrap_or_default();
    let stem: String = file
        .rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(MAX_STEM)
        .collect();
    format!(r"\\.\pipe\kelpie-{stem}-{digest}")
}

/// Connects to the endpoint `socket` names
///
/// # Errors
///
/// Whatever the OS says, such as nothing listening there.
pub async fn connect(socket: &Path) -> io::Result<ClientStream> {
    transport::connect(&address(socket)).await
}

/// Where connections to an endpoint arrive
#[derive(Debug)]
pub struct Listener(transport::Listener);

impl Listener {
    /// Listens at the endpoint `socket` names
    ///
    /// # Errors
    ///
    /// Whatever the OS says: on Unix the path is taken or cannot be written,
    /// on Windows the pipe is already served.
    pub fn bind(socket: &Path) -> io::Result<Self> {
        transport::Listener::bind(&address(socket)).map(Self)
    }

    /// The next connection
    ///
    /// # Errors
    ///
    /// Whatever the OS says. The caller decides whether to go on.
    pub async fn accept(&mut self) -> io::Result<ServerStream> {
        self.0.accept().await
    }
}
