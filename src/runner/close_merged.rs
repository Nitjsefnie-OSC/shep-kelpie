//! Closing the issue of a merged pull request that left it open
//!
//! A pull request merged with no closing reference in its body leaves its
//! issue open, however the merge came about, and anything the issue blocks
//! then waits on it. Once a work item's merge is recorded, a step reads the
//! issue and closes it if the forge still shows it open.

use super::Runner;

/// Closes in a row the forge may refuse before the step gives up on an issue
const TRIES: u32 = 3;

/// What kelpie says on an issue it closes after the pull request merged
/// without closing it
fn merged_without_closing(pull_request: u64) -> String {
    format!("Pull request #{pull_request} merged without closing this issue, so kelpie closes it.")
}

impl Runner {
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
