//! The files a branch changes from `main`

use std::path::Path;

use super::{BASE, WorktreeError, git};

/// The files `head` changes from `origin/main`, as the pull request shows them
///
/// # Errors
///
/// [`WorktreeError`] naming the git command that failed.
pub fn changed_files(repo: &Path, head: &str) -> Result<Vec<String>, WorktreeError> {
    let range = format!("origin/{BASE}...{head}");
    let names = git(repo, ["diff", "--name-only", "--no-renames", "-z", &range])?;
    Ok(names
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}
