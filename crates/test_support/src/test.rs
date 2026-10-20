use std::time::Duration;

mod repo;
mod script;
mod tempdir;

pub use repo::{git, linked_worktree, write_in};
pub use script::write_script;
pub use tempdir::short_tempdir;

/// How long a test waits on a call, a socket or a thread that runs on real
/// time before it fails by name
pub const CALL_PATIENCE: Duration = Duration::from_secs(60);
