//! Closing the issue of a merged pull request that left it open
//!
//! A pull request merged with no closing reference in its body leaves its
//! issue open, however the merge came about, and anything the issue blocks
//! then waits on it. Once a work item's merge is recorded, a step reads the
//! issue and closes it if the forge still shows it open.

use super::Runner;

/// Closes the forge may refuse, one in each step, before the step gives up on an issue
const TRIES: u32 = 3;

/// What kelpie says on an issue it closes after the pull request merged
/// without closing it
fn merged_without_closing(pull_request: u64) -> String {
    format!("Pull request #{pull_request} merged without closing this issue, so kelpie closes it.")
}

impl Runner {
    /// Closes `issue` if the forge still shows it open after `pull_request`
    /// merged, trying the close once
    ///
    /// The comment goes first, once, as `close_done_parent` does. A close the
    /// forge refuses returns why, for the step to fail on and run again after
    /// its backoff, until it has been refused [`TRIES`] times: then it is told
    /// in the log and the work item goes on. A comment that fails or an issue
    /// that cannot be read is told in the log and never holds the work item.
    pub(super) fn close_merged_issue(&mut self, issue: u64, pull_request: u64) -> Option<String> {
        // The forge closes a named issue a moment after the merge, so one that
        // still reads as open then is left to it.
        let named = (self.ports.forge).reviewed(&self.remote, pull_request);
        if named.is_ok_and(|pr| pr.closes.contains(&issue)) {
            return None;
        }
        match (self.ports.forge).issue(&self.remote, issue) {
            Ok(read) if !read.open => {
                self.close_refused.remove(&issue);
                return None;
            }
            Ok(_) => {}
            Err(e) => {
                self.notes.push(format!(
                    "cannot read issue #{issue} after merging #{pull_request}: {e}"
                ));
                return None;
            }
        }
        if !self.close_refused.contains_key(&issue)
            && let Err(e) = self.comment_on_issue(issue, &merged_without_closing(pull_request))
        {
            self.notes.push(format!(
                "cannot comment on issue #{issue} before closing it: {e}"
            ));
        }
        let Err(e) = (self.ports.forge).close_issue(&self.remote, issue) else {
            self.close_refused.remove(&issue);
            return None;
        };
        let failures = self.close_refused.entry(issue).or_default();
        *failures += 1;
        let reason = format!("cannot close issue #{issue} after merging #{pull_request}: {e}");
        if *failures < TRIES {
            return Some(reason);
        }
        self.close_refused.remove(&issue);
        self.notes.push(format!("{reason}, refused {TRIES} times"));
        None
    }
}
