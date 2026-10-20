//! Dev-only helpers for tests: temp folders, stand-in scripts and git
//!
//! Each one names nothing but std and `tempfile`, so a test in any crate can
//! use them. Uses no other member of the workspace, and is named only in
//! `[dev-dependencies]`.

#![forbid(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

// The folder the helpers sit in, so a test's path reads `test::tempdir::tests::...`
// as it did before they moved here
mod test;

pub use test::{CALL_PATIENCE, git, linked_worktree, short_tempdir, write_in, write_script};
