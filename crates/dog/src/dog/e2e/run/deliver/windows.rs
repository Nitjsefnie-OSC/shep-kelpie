//! The copy on Windows, which waits on #1302
//!
//! There is no `openat` here to hold each folder open while the next is
//! opened from it, so a link could be put in the way between a check and the
//! write. Until #1302 gives Windows a copy that refuses one, nothing is
//! copied.

use std::path::Path;

/// Refuses, so that no result is written through a link
///
/// # Errors
///
/// Always: a message naming #1302.
pub(in crate::dog::e2e::run) fn deliver(
    _staging: &Path,
    _worktree: &Path,
    _results: &Path,
) -> Result<(), String> {
    Err(UNSUPPORTED.to_owned())
}

const UNSUPPORTED: &str =
    "unsupported: results are not copied into the worktree on Windows yet (#1302)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_results_are_refused_with_the_issue_that_will_copy_them() {
        let left = deliver(
            Path::new("staging"),
            Path::new("worktree"),
            Path::new("out"),
        );
        assert!(left.unwrap_err().contains("#1302"));
    }
}
