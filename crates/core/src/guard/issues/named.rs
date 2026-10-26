//! The paths an issue names, checked against the checkout of `main` before it is filed
//!
//! A path in backticks that matches no file there goes back to the issue writer once:
//! the first time it is named, the filing is refused with the paths, and the ledger
//! remembers them, so the same issue filed again goes through. A path the issue adds
//! is among them, so the refusal asks the writer to keep those.

use std::fs;
use std::path::Path;

use super::gh::Gh;
use crate::board::briefing::missing_paths;

// The most files read from a checkout, so a vast one cannot hold the hook up.
const MOST: usize = 200_000;

/// Refuses `body` when it names paths that are on no file of the checkout, and not
/// yet sent back, and remembers them
///
/// A session with no ledger, or no checkout to read, is let through: nothing could
/// remember what was sent back.
pub(super) fn check(body: &str, gh: &Gh<'_>) -> Result<(), String> {
    let Some(ledger) = gh.rules.ledger.as_deref() else {
        return Ok(());
    };
    let tree = files_under(gh.checkout);
    if tree.is_empty() {
        return Ok(());
    }
    let sent: Vec<String> = (missing_paths(body, &tree).into_iter())
        .filter(|path| !gh.ledger.told(path))
        .collect();
    if sent.is_empty() {
        return Ok(());
    }
    super::ledger::tell(ledger, &sent);
    let named: Vec<String> = sent.iter().map(|path| format!("`{path}`")).collect();
    Err(format!(
        "these name no file on main: {}. Search the checkout for each with Grep or Glob. Correct \
         a path that moved or was renamed, and keep one this issue adds, marked as new. Then \
         file the issue again: these paths are not checked a second time.",
        named.join(", ")
    ))
}

// Every file under `root`, as a path from it, without the repo's own `.git`.
fn files_under(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut folders = vec![(root.to_owned(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            let path = format!("{prefix}{name}");
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push((entry.path(), format!("{path}/"))),
                Ok(_) => files.push(path),
                Err(_) => {}
            }
            if files.len() >= MOST {
                return files;
            }
        }
    }
    files
}
