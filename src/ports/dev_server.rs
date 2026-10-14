//! The dev servers port: a project's dev server, started for one worker turn
//!
//! A worker's sandbox binds no port and a headless turn has no background
//! tasks, so the worker cannot keep a dev server running itself. Kelpie starts
//! the project's, from its launch configuration, in a sandbox of its own, and
//! stops it when the turn ends.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use super::Policy;

/// How long a dev server gets to answer on its port once started
pub const READY_WITHIN: Duration = Duration::from_secs(60);

/// One dev server to start
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevRun {
    /// The program, as the launch configuration's `runtimeExecutable` names it
    pub program: String,
    /// Its arguments, with the port settled
    pub args: Vec<String>,
    /// The folder it starts in, inside the worktree
    pub cwd: PathBuf,
    /// The port it is to answer on, on this machine
    pub port: u16,
    /// Variables set for it
    pub env: BTreeMap<String, String>,
    /// What it may write, read and reach
    pub policy: Policy,
    /// Where the sandbox's settings for it are written, a file it cannot write
    pub settings: PathBuf,
    /// The file its output goes to, in the build folder
    pub log: PathBuf,
    /// How long it gets to answer on `port`
    pub ready_within: Duration,
}

/// A dev server that was started, stopped by being dropped
///
/// Its process group ends with it, whatever ended the turn.
pub struct Started {
    /// Whether it answered on its port within the wait
    pub answered: bool,
    _held: Box<dyn Send + Sync>,
}

impl Started {
    /// A server that `answered` or not, which `held` stops when it is dropped
    pub fn new(answered: bool, held: impl Send + Sync + 'static) -> Self {
        Self {
            answered,
            _held: Box::new(held),
        }
    }
}

impl fmt::Debug for Started {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Started")
            .field("answered", &self.answered)
            .finish_non_exhaustive()
    }
}

/// Starts a project's dev server
pub trait DevServers: Send + Sync {
    /// Whether nothing on this machine holds `port`
    fn free(&self, port: u16) -> bool;

    /// A port nothing holds
    ///
    /// # Errors
    ///
    /// The reason, when none can be had.
    fn pick(&self) -> Result<u16, String>;

    /// Starts `run` and waits for it to answer on its port, up to its wait
    ///
    /// A server that never answers is still returned, with `answered` false,
    /// so the turn runs and its worker is told. The server is kept running
    /// until the [`Started`] is dropped.
    ///
    /// # Errors
    ///
    /// The reason, when the sandbox or the process could not be started.
    fn start(&self, run: &DevRun) -> Result<Started, String>;
}
