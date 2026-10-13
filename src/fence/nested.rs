//! The harness folders already below a worktree, for a sandbox that cannot fence them by glob

use std::fs;
use std::path::{Path, PathBuf};

use super::{HARNESSES, folds_to};

// Deep enough for a monorepo's packages, shallow enough that a link loop or a
// vendored tree costs little.
const DEPTH: usize = 8;
const ENTRIES: usize = 20_000;

/// Every harness folder below `worktree`, in any case, as a path, sorted
///
/// The walk is best effort: a folder it cannot read, a link it does not follow
/// and whatever lies past [`DEPTH`] levels or [`ENTRIES`] entries are left out,
/// and so is `.git`. A folder not found is caught by the check against `main`
/// after the call. A folder that holds one is listed whole and not entered.
pub(super) fn existing(worktree: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![(worktree.to_owned(), 0)];
    let mut seen = 0;
    while let Some((folder, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > ENTRIES {
                found.sort();
                return found;
            }
            let name = entry.file_name();
            if (HARNESSES.iter().flat_map(|own| own.folders)).any(|own| folds_to(&name, own)) {
                found.push(entry.path());
            } else if depth < DEPTH
                && name != ".git"
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    found.sort();
    found
}
