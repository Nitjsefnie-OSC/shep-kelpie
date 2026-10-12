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
    let name = file.display();
    match (written, removed) {
        (Some(Err(w)), Err(r)) => Some(format!(
            "cannot write {name}: {w}, and cannot remove the stale file: {r}"
        )),
        (Some(Err(w)), Ok(())) => Some(format!("cannot write {name}: {w}")),
        (_, Err(r)) => Some(format!("cannot remove {name}: {r}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_can_be_neither_written_nor_removed_says_both() {
        let dir = tempfile::tempdir().unwrap();
        // A folder with something in it stands where the file should be.
        let file = dir.path().join("e2e.json");
        fs::create_dir_all(file.join("held")).unwrap();
        let e2e: E2e = serde_json::from_str(r#"{"host":"e2e-box"}"#).unwrap();

        let note = record(Some(&e2e), &file).expect("a failure is noted");

        assert!(note.contains("cannot write"), "{note}");
        assert!(note.contains("cannot remove the stale file"), "{note}");
    }
}
