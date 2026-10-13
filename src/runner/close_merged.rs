//! Closing the issue of a merged pull request that left it open
//!
//! A pull request merged with no closing reference in its body leaves its
//! issue open, however the merge came about, and anything the issue blocks
//! then waits on it. Once a work item's merge is recorded, a step reads the
//! issue and closes it if the forge still shows it open.

use super::Runner;
use crate::ports::{SubIssue, SubIssueState};

/// Closes in a row the forge may refuse before the step gives up on an issue
const TRIES: u32 = 3;

/// What kelpie says on an issue it closes after the pull request merged
/// without closing it
fn merged_without_closing(pull_request: u64) -> String {
    format!("Pull request #{pull_request} merged without closing this issue, so kelpie closes it.")
}

/// What kelpie says on a parent it closes after the pull request that closed
/// its last open sub-issue merged
fn last_sub_issue_merged(pull_request: u64) -> String {
    format!(
        "Pull request #{pull_request} closed the last open sub-issue of this issue, so kelpie closes it too."
    )
}

impl Runner {
    /// Closes what merging `pull_request` for `issue` leaves open: the issue
    /// itself, then its parent
    pub(super) fn close_after_merge(&mut self, issue: u64, pull_request: u64) {
        self.close_merged_issue(issue, pull_request);
        self.close_done_parent_of(issue, pull_request);
    }

    /// Closes `issue` if the forge still shows it open after `pull_request`
    /// merged, never holding the work item
    ///
    /// The comment goes first, once, as `close_done_parent` does. One that
    /// fails, a close the forge refuses [`TRIES`] times, or an issue that
    /// cannot be read is told in the log, and the work item goes on.
    pub(super) fn close_merged_issue(&mut self, issue: u64, pull_request: u64) {
        // The forge closes a named issue a moment after the merge, so one that
        // still reads as open then is left to it.
        let named = (self.ports.forge).reviewed(&self.remote, pull_request);
        if named.is_ok_and(|pr| pr.closes.contains(&issue)) {
            return;
        }
        match (self.ports.forge).issue(&self.remote, issue) {
            Ok(read) if !read.open => return,
            Ok(_) => {}
            Err(e) => {
                self.notes.push(format!(
                    "cannot read issue #{issue} after merging #{pull_request}: {e}"
                ));
                return;
            }
        }
        if let Err(e) = self.comment_on_issue(issue, &merged_without_closing(pull_request)) {
            self.notes.push(format!(
                "cannot comment on issue #{issue} before closing it: {e}"
            ));
        }
        let mut refused = None;
        for _ in 0..TRIES {
            match (self.ports.forge).close_issue(&self.remote, issue) {
                Ok(()) => return,
                Err(e) => refused = Some(e),
            }
        }
        if let Some(e) = refused {
            self.notes.push(format!(
                "cannot close issue #{issue} after merging #{pull_request}, tried {TRIES} times: {e}"
            ));
        }
    }
}

impl Runner {
    /// Closes the parent of `issue` if `pull_request`'s merge left every one of
    /// its sub-issues closed and at least one of them built, never holding the
    /// work item
    ///
    /// `issue` counts as completed while the forge still reads it open, since
    /// it closes a moment after the merge. Only the parent is closed: a grandparent waits
    /// for its own next merge. A read or a close that fails is told in the log.
    pub(super) fn close_done_parent_of(&mut self, issue: u64, pull_request: u64) {
        let parent = match (self.ports.forge).issue(&self.remote, issue) {
            Ok(read) => read.parent,
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the parent of issue #{issue} after merging #{pull_request}: {e}"
                ));
                return;
            }
        };
        let Some(parent) = parent else { return };
        match (self.ports.forge).issue(&self.remote, parent) {
            Ok(read) if !read.open => return,
            Ok(_) => {}
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the parent #{parent} of issue #{issue} after merging #{pull_request}: {e}"
                ));
                return;
            }
        }
        let subs = match (self.ports.forge).sub_issues(&self.remote, parent) {
            Ok(subs) => subs,
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the sub-issues of #{parent} after merging #{pull_request}: {e}"
                ));
                return;
            }
        };
        let state = |s: &SubIssue| match (s.number == issue, s.state) {
            (true, SubIssueState::Open) => SubIssueState::Completed,
            _ => s.state,
        };
        let done = subs.iter().all(|s| state(s) != SubIssueState::Open);
        let built = subs.iter().any(|s| state(s) == SubIssueState::Completed);
        if !(done && built) {
            return;
        }
        if let Err(e) = self.comment_on_issue(parent, &last_sub_issue_merged(pull_request)) {
            self.notes.push(format!(
                "cannot comment on issue #{parent} before closing it: {e}"
            ));
        }
        if let Err(e) = (self.ports.forge).close_issue(&self.remote, parent) {
            self.notes.push(format!(
                "cannot close issue #{parent} after merging #{pull_request}: {e}"
            ));
        }
    }
}
