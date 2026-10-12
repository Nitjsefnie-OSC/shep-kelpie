//! The project's e2e table, written for the dog
//!
//! A worker's fence writes only its worktree and build folder, so it cannot
//! change `e2e.json` in the project's folder. The dog reads it, not the ask,
//! to learn what host a worker's run goes to.

use std::fs;
use std::io;
use std::path::Path;

use crate::settings::E2e;
use crate::state::write_atomically;

/// Writes `e2e` to `file`, or removes a stale `file` when there is no table
///
/// Says what went wrong in a line. A file that cannot be written is removed
/// too, so the dog never reads a host the settings no longer name.
pub(super) fn record(e2e: Option<&E2e>, file: &Path) -> Option<String> {
    let written = e2e.map(|e2e| {
        serde_json::to_vec(e2e)
            .map_err(io::Error::other)
            .and_then(|bytes| write_atomically(file, &bytes))
    });
    if let Some(Ok(())) = written {
        return None;
    }
    let removed = match fs::remove_file(file) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    match (written, removed) {
        (Some(Err(e)), _) => Some(format!("cannot write {}: {e}", file.display())),
        (_, Err(e)) => Some(format!("cannot remove {}: {e}", file.display())),
        _ => None,
    }
}
