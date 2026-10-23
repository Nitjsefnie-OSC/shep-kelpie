//! [`super`] for threads that block: std's own types on Unix, pipes driven by a
//! private runtime on Windows
//!
//! A bridge or a forwarder serves each connection on a thread of its own, with
//! read timeouts, clones of the stream and a shutdown that wakes a blocked
//! reader. A Unix socket does all of that as std has it. A named pipe has no
//! std server end, so on Windows [`Stream`] and [`Listener`] keep the same
//! calls over tokio's, run on a thread that belongs to this module.

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

#[cfg(windows)]
pub use pipe::{Listener, Stream};

#[cfg(windows)]
mod pipe;
