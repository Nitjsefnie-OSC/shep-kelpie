//! Closing the issue of a merged pull request that left it open
//!
//! A pull request merged with no closing reference in its body leaves its
//! issue open, however the merge came about, and anything the issue blocks
//! then waits on it. Once a work item's merge is recorded, a step reads the
//! issue and closes it if the forge still shows it open.

use super::Runner;
use crate::ports::{SubIssue, SubIssueState};
use crate::ticket::TicketId;

/// Closes the forge may refuse, one in each step, before the step gives up on an issue
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
    /// Closes `issue` if the forge still shows it open after `pull_request`
    /// merged, trying the close once
    ///
    /// The comment goes first, once, as `close_done_parent` does. A close the
    /// forge refuses returns why, for the step to fail on and run again after
    /// its backoff, until it has been refused [`TRIES`] times: then it is told
    /// in the log and the work item goes on. A comment that fails or an issue
    /// that cannot be read is told in the log and never holds the work item.
    pub(super) fn close_merged_issue(
        &mut self,
        ticket: TicketId,
        pull_request: u64,
    ) -> Option<String> {
        // Only a GitHub issue has a pull request to merge, so a ticket with no
        // GitHub number is nothing this closes.
        let issue = ticket.github()?;
        // The forge closes a named issue a moment after the merge, so one that
        // still reads as open then is left to it.
        let named = (self.ports.forge).reviewed(&self.remote, pull_request);
        if named.is_ok_and(|pr| pr.closes.contains(&issue)) {
            return None;
        }
        match self.ports.tickets.issue(&self.remote, ticket) {
            Ok(read) if !read.open => {
                self.close_refused.remove(&ticket);
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
        if !self.close_refused.contains_key(&ticket)
            && let Err(e) = self.comment_on_issue(issue, &merged_without_closing(pull_request))
        {
            self.notes.push(format!(
                "cannot comment on issue #{issue} before closing it: {e}"
            ));
        }
        let Err(e) = self.ports.tickets.close_issue(&self.remote, ticket) else {
            self.close_refused.remove(&ticket);
            return None;
        };
        let failures = self.close_refused.entry(ticket).or_default();
        *failures += 1;
        let reason = format!("cannot close issue #{issue} after merging #{pull_request}: {e}");
        if *failures < TRIES {
            return Some(reason);
        }
        self.close_refused.remove(&ticket);
        self.notes.push(format!("{reason}, refused {TRIES} times"));
        None
    }
}

impl Runner {
    /// Closes the parent of `issue` if `pull_request`'s merge left every one of
    /// its sub-issues closed and at least one of them built, never holding the
    /// work item
    ///
    /// `issue` and each issue the pull request names count as completed while
    /// the forge still reads them open, since it closes them a moment after the
    /// merge. Only the parent is closed: a grandparent waits
    /// for its own next merge. A read or a close that fails is told in the log.
    pub(super) fn close_done_parent_of(&mut self, ticket: TicketId, pull_request: u64) {
        let parent = match self.ports.tickets.issue(&self.remote, ticket) {
            Ok(read) => read.parent,
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the parent of issue {} after merging #{pull_request}: {e}",
                    ticket.shown()
                ));
                return;
            }
        };
        let Some(parent) = parent else { return };
        match self.ports.tickets.issue(&self.remote, parent) {
            Ok(read) if !read.open => return,
            Ok(_) => {}
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the parent #{parent} of issue {} after merging #{pull_request}: {e}",
                    ticket.shown()
                ));
                return;
            }
        }
        let subs = match self.ports.tickets.sub_issues(&self.remote, parent) {
            Ok(subs) => subs,
            Err(e) => {
                self.notes.push(format!(
                    "cannot read the sub-issues of #{parent} after merging #{pull_request}: {e}"
                ));
                return;
            }
        };
        // A read that fails leaves only `issue` to count, so the parent may stay
        // open until a later merge under it.
        let named = (self.ports.forge).reviewed(&self.remote, pull_request);
        let named: Vec<TicketId> = (named.map(|pr| pr.closes).unwrap_or_default().into_iter())
            .filter_map(TicketId::from_issue)
            .collect();
        let state = |s: &SubIssue| match (s.ticket == ticket || named.contains(&s.ticket), s.state)
        {
            (true, SubIssueState::Open) => SubIssueState::Completed,
            _ => s.state,
        };
        let done = subs.iter().all(|s| state(s) != SubIssueState::Open);
        let built = subs.iter().any(|s| state(s) == SubIssueState::Completed);
        if !(done && built) {
            return;
        }
        if let Err(e) = self.comment_on_issue(
            parent.github().unwrap_or_default(),
            &last_sub_issue_merged(pull_request),
        ) {
            self.notes.push(format!(
                "cannot comment on issue #{parent} before closing it: {e}"
            ));
        }
        if let Err(e) = self.ports.tickets.close_issue(&self.remote, parent) {
            self.notes.push(format!(
                "cannot close issue #{parent} after merging #{pull_request}: {e}"
            ));
        }
    }
}
