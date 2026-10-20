//! The bridge port: the browser's MCP server behind one socket, open for one turn
//!
//! The runner opens a bridge when it prepares a worker's turn and holds it until the turn ends
//! (ADR 0011). What serves the socket, and how the browser is held to the project's hosts, is
//! the adapter's.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::bridge::Filter;

/// The Playwright MCP server a bridge serves, and the Chromium it launches
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playwright {
    /// The server's entry point
    pub mcp: PathBuf,
    /// Where the Chromium it launches is
    pub browsers: PathBuf,
    /// A folder of kelpie's own, where the server's config, output and log go
    pub folder: PathBuf,
    /// Whether the browser opens a window, rather than running headless
    pub headed: bool,
}

/// A bridge that is open: it serves its socket until it is dropped
pub trait OpenBridge: Send + fmt::Debug {
    /// The socket the call connects to
    fn socket(&self) -> &Path;
}

/// Opens the bridge a worker's turn reaches its browser through
pub trait Bridges: Send + Sync {
    /// Opens a socket in `worker`, served by `server` through `filter`
    ///
    /// The socket's name is the one `name_file` holds, drawn and written there if it holds none, so
    /// a later bridge for the same work item binds the same path. Dropping the returned bridge
    /// stops the server and removes the socket.
    ///
    /// # Errors
    ///
    /// The reason, when the bridge cannot be opened.
    fn open(
        &self,
        worker: &Path,
        name_file: &Path,
        server: &Playwright,
        filter: Filter,
    ) -> Result<Box<dyn OpenBridge>, String>;
}
