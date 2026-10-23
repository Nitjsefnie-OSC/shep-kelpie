use std::time::Duration;

mod elsewhere;
mod fake;
mod repo;
mod script;
mod status;
mod tempdir;

pub use elsewhere::Elsewhere;
pub use fake::{Fake, write_fake};
pub use repo::{cut_from_main, git, linked_worktree, main_commits, write_in};
pub use script::{make_fifo, waits_for, write_script};
pub use status::exited;
#[cfg(unix)]
pub use status::signalled;
pub use tempdir::short_tempdir;

/// How long a test waits on a call, a socket or a thread that runs on real
/// time before it fails by name
pub const CALL_PATIENCE: Duration = Duration::from_secs(60);
