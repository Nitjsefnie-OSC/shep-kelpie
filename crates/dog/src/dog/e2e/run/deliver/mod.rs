//! Putting the results the host sent into the worktree, past no link
//!
//! rsync writes the results into a folder of the dog's own, and this copies
//! them from there. The worktree is the worker's to change while the copy
//! goes, so a path checked and then used could be a link by then. Each folder
//! is instead opened from the one before it with `O_NOFOLLOW`, and each file
//! is created through the folder it is in, so a link anywhere on the way is
//! refused and nothing is written through it. A file already there is
//! unlinked and made again, never opened: a hard link would be written
//! through, and a FIFO would block the dog.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(super) use unix::deliver;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::deliver;
