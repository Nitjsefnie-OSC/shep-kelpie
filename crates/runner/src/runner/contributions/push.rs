//! Pushing a fork worker's commits to the contributor's branch
//!
//! A worker on a contributor's fork only commits, and the maintainer's login,
//! which can push to the fork, must never be in its reach. So the runner
//! pushes, outside the sandbox, with the login git holds there: after each
//! worker turn, and at the CI gate, which tries a push that was refused again
//! once the maintainer says yes. The guard and profile refuse the worker's own
//! push as guardrails; its token, which #1134 measured able to push to a fork
//! while edits are on, is the boundary.
//!
//! The push is fast-forward only and never forced. A fork head that is not the
//! one kelpie last knew is the contributor's own push: nothing is pushed, and
//! the gate raises the foreign-change ruling, so the commits wait unpushed. A
//! push the fork refuses (edits turned off since, the branch deleted) raises
//! a [`Stuck::PushRefused`] ruling naming git's reason, and the commits stay.

use super::super::Runner;
use super::super::report::Begin;
use crate::ports::PullRequestState;
use crate::state::{StateError, Stuck};
use crate::worktree;

/// What pushing the worktree's commits to the work item's fork came to
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runner) enum Sent {
    /// There was nothing to push: not a fork, or the fork holds the head
    Nothing,
    /// The fork's branch now holds this commit
    Pushed(String),
    /// The fork's head is not the one kelpie knew, so the contributor pushed
    /// to it and the commits wait
    Moved,
    /// The pull request was merged or closed by hand, so its branch takes no
    /// push and the gate rules on it
    Ended,
    /// The fork refused, or could not be reached, for this reason
    Refused(String),
}

impl Runner {
    /// Pushes the worktree's commits to the fork's branch when the work item's
    /// remote is a fork whose head is still the one kelpie knew
    pub(in crate::runner) fn push_to_fork(&self) -> Sent {
        let Some(item) = self.current() else {
            return Sent::Nothing;
        };
        if item.remote.is_origin() {
            return Sent::Nothing;
        }
        // A pull request merged or closed by hand takes no push: its branch
        // may be gone, and that is not a refusal. The gate rules on it.
        if let Some(number) = item.pull_request
            && matches!(self.pull_request_view(number), Ok(pr) if pr.state != PullRequestState::Open)
        {
            return Sent::Ended;
        }
        let repo = &self.settings.git.checkout;
        let local = match worktree::head(repo, &item.worktree) {
            Ok(local) => local,
            Err(e) => {
                eprintln!("cannot read issue #{}'s worktree head: {e}", item.issue);
                return Sent::Nothing;
            }
        };
        // The fork is read even where the worktree holds nothing new: the
        // contributor may have pushed since, and only the gate can rule on it.
        let theirs = match self.origin_head() {
            Ok(theirs) => theirs,
            Err(reason) => return Sent::Refused(reason),
        };
        if theirs == local {
            return Sent::Nothing;
        }
        if item.known.head.as_deref() != Some(theirs.as_str()) {
            return Sent::Moved;
        }
        match worktree::push_commits(repo, &item.worktree, &item.remote) {
            Ok(()) => Sent::Pushed(local),
            Err(e) => Sent::Refused(e.to_string()),
        }
    }

    /// The CI gate's push of a fork worker's commits: told to the work item as
    /// kelpie's last known head when it went, or the ruling that parks it
    ///
    /// `Ok(None)` lets the gate go on.
    pub(in crate::runner) fn send_commits(
        &mut self,
        number: u64,
    ) -> Result<Option<Begin>, StateError> {
        let sent = self.push_to_fork();
        self.sent(number, sent)
    }

    /// What a push that came to `sent` does to the work item
    pub(in crate::runner) fn sent(
        &mut self,
        number: u64,
        sent: Sent,
    ) -> Result<Option<Begin>, StateError> {
        match sent {
            Sent::Pushed(head) => {
                // The push moved the fork's head, so a head fetched before it is stale
                self.reads.git.clear();
                self.update(|item| item.known.head = Some(head))?;
                Ok(None)
            }
            Sent::Refused(why) => self
                .raise(number, Stuck::PushRefused { why }.into())
                .map(Some),
            // A moved head is the gate's to raise: it reads the fork's head.
            Sent::Nothing | Sent::Moved | Sent::Ended => Ok(None),
        }
    }
}
