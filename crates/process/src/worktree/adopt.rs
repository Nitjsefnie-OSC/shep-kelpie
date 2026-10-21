//! Bringing a worktree to a head the maintainer accepted

use std::path::Path;

use super::{Remote, WorktreeError, trusted};

/// Moves the worktree's branch from `from`, the head kelpie knew, to `to`
///
/// `to` is a head on `remote` the maintainer accepted. A worktree at neither,
/// or with changes not committed, holds work of the worker's, and is refused.
/// On a fork the worker's own commits, which kelpie has not pushed, are kept:
/// where they sit on `from`, `to` is merged into them, and a merge that
/// conflicts is abandoned and refused.
///
/// # Errors
///
/// [`WorktreeError`] naming the git command that failed, or
/// [`WorktreeError::Unsettled`] for a worktree refused.
pub fn adopt(
    repo: &Path,
    worktree: &Path,
    branch: &str,
    from: &str,
    to: &str,
    remote: &Remote,
) -> Result<(), WorktreeError> {
    let in_worktree = trusted(repo, worktree)?;
    let unsettled = || WorktreeError::Unsettled(worktree.to_owned());
    let full_ref = format!("refs/heads/{branch}");
    let on_branch = in_worktree(&["symbolic-ref", "--quiet", "HEAD"])
        .ok()
        .as_deref()
        == Some(&full_ref);
    if !on_branch || !in_worktree(&["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err(unsettled());
    }
    let at = in_worktree(&["rev-parse", "HEAD"])?;
    if at == to {
        return Ok(());
    }
    let commits_kept = at != from
        && matches!(remote, Remote::Fork { .. })
        && in_worktree(&["merge-base", "--is-ancestor", from, &at]).is_ok();
    if at != from && !commits_kept {
        return Err(unsettled());
    }
    remote.fetch(repo, branch)?;
    if !commits_kept {
        in_worktree(&["reset", "--quiet", "--hard", to])?;
        return Ok(());
    }
    // The merge commit takes the worker's committer, so it needs no identity
    // of its own.
    let name = format!("user.name={}", in_worktree(&["log", "-1", "--format=%cn"])?);
    let email = format!(
        "user.email={}",
        in_worktree(&["log", "-1", "--format=%ce"])?
    );
    let merged = in_worktree(&[
        "-c",
        &name,
        "-c",
        &email,
        "merge",
        "--quiet",
        "--no-edit",
        "--no-ff",
        to,
    ]);
    if merged.is_err() {
        // Best effort: a merge left in progress is refused on the next look.
        let _ = in_worktree(&["merge", "--abort"]);
        return Err(unsettled());
    }
    Ok(())
}
