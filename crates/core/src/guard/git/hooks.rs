//! The project's `commit` and `push` hooks, run on what the guard let through
//!
//! The guard has read the commit's message and the lines it adds, or the
//! commits a push sends, and found nothing of this machine's. The hooks
//! judge the same, once per command, with the staged diff or the commits as
//! the worktree's own git, trusted, holds them. Git that cannot be read
//! refuses: a check that cannot run must not pass silently.

use serde_json::{Value, json};

use super::PLAIN;
use crate::guard::{Checkout, Home};
use crate::hooks::Role;
use crate::settings::HookPoint;
use crate::worktree::{self, WorktreeError};

/// Runs the `commit` hooks on a commit with `messages` in the worktree, of
/// every tracked change with `all`, else of what is staged
pub(super) fn commit(
    home: &Home,
    checkout: Checkout<'_>,
    messages: &[String],
    all: bool,
) -> Vec<String> {
    if !home.hooks.has(HookPoint::Commit) {
        return Vec::new();
    }
    let read = |run: &dyn Fn(&[&str]) -> Result<String, WorktreeError>| {
        let range = if all { "HEAD" } else { "--cached" };
        let diff = run(&[&["diff", range][..], &PLAIN].concat())?;
        Ok(json!({ "message": messages.join("\n\n"), "messages": messages, "diff": diff }))
    };
    judged("commit", checkout, home, HookPoint::Commit, read)
}

/// Runs the `push` hooks on the commits each of `sources` sends past
/// `origin`'s base branch
pub(super) fn push(home: &Home, checkout: Checkout<'_>, sources: &[String]) -> Vec<String> {
    if !home.hooks.has(HookPoint::Push) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for source in sources {
        let read = |run: &dyn Fn(&[&str]) -> Result<String, WorktreeError>| {
            Ok(crate::hooks::commits_past_base(run, source, checkout.base)?.unwrap_or(Value::Null))
        };
        out.extend(judged("push", checkout, home, HookPoint::Push, read));
    }
    out
}

// Reads the point's fields from the worktree's git, then runs its hooks on
// them. A `null` read has nothing to judge.
fn judged(
    what: &str,
    checkout: Checkout<'_>,
    home: &Home,
    point: HookPoint,
    read: impl FnOnce(&dyn Fn(&[&str]) -> Result<String, WorktreeError>) -> Result<Value, WorktreeError>,
) -> Vec<String> {
    let details =
        worktree::trusted(checkout.git_common_dir, checkout.worktree).and_then(|run| read(&run));
    let details = match details {
        Ok(Value::Null) => return Vec::new(),
        Ok(details) => details,
        Err(e) => {
            return vec![format!(
                "kelpie cannot read this worktree's git to run the project's {what} hook: {e}"
            )];
        }
    };
    match home.hooks.check(point, Role::Worker, None, details) {
        Ok(()) => Vec::new(),
        Err(refusal) => vec![format!(
            "this {what}: {refusal}. Change it so the hook accepts it, then try again."
        )],
    }
}
