//! The git calls that change a work item's worktree, made with the runner's lock let go
//!
//! A rebase pushes and a removal deletes, so either can hold the lock for seconds. A step returns
//! the call as a job (`turn/reads.rs`) and the rotation applies what it found when it takes the
//! lock again: the rest of the step runs from the answer, so a push and its save keep their order
//! and nothing the call moved is read again. While the job runs the runner marks its work item
//! busy (`off_lock::Busy`), and a trigger on its worktree waits for the apply.

use std::path::PathBuf;

use super::Runner;
use super::report::Begin;
use crate::state::StateError;
use crate::work_item::Review;
use crate::worktree::{self, Rebase, Remote, WorktreeError};

/// A call that changes a worktree, made from owned values
pub(in crate::runner) enum Change {
    Rebase(Rebasing),
    Remove(Removing),
}

/// What a [`Change`] came to, to apply to the work item it was about
pub(in crate::runner) enum Changed {
    Rebased(Caught),
    Removed(Cleared),
}

/// The worktree's branch, at `head`, caught up with its base and pushed
pub(in crate::runner) struct Rebasing {
    pub(in crate::runner) checkout: PathBuf,
    pub(in crate::runner) worktree: PathBuf,
    pub(in crate::runner) branch: String,
    pub(in crate::runner) head: String,
    pub(in crate::runner) rewrite: bool,
    pub(in crate::runner) remote: Remote,
    pub(in crate::runner) base: String,
    pub(in crate::runner) number: u64,
    pub(in crate::runner) review: Option<Review>,
}

/// What catching a branch up came to
pub(in crate::runner) struct Caught {
    pub(in crate::runner) number: u64,
    pub(in crate::runner) head: String,
    pub(in crate::runner) review: Option<Review>,
    pub(in crate::runner) outcome: Result<Rebase, WorktreeError>,
}

/// A work item's worktree, branch and build folder removed
pub(in crate::runner) struct Removing {
    pub(in crate::runner) checkout: PathBuf,
    pub(in crate::runner) worktree: PathBuf,
    pub(in crate::runner) branch: String,
    pub(in crate::runner) build: PathBuf,
    pub(in crate::runner) remote: Option<Remote>,
    pub(in crate::runner) merged: bool,
    // Read before the build folder goes, which holds the file it is in
    pub(in crate::runner) friction: Option<String>,
}

/// What a removal came to
pub(in crate::runner) struct Cleared {
    pub(in crate::runner) merged: bool,
    pub(in crate::runner) friction: Option<String>,
    pub(in crate::runner) outcome: Result<(), WorktreeError>,
}

impl Change {
    /// Makes the call
    pub(in crate::runner) fn run(self) -> Changed {
        match self {
            Self::Rebase(ask) => Changed::Rebased(Caught {
                outcome: worktree::rebase(
                    &ask.checkout,
                    &ask.worktree,
                    &ask.branch,
                    &ask.head,
                    ask.rewrite,
                    &ask.remote,
                    &ask.base,
                ),
                number: ask.number,
                head: ask.head,
                review: ask.review,
            }),
            Self::Remove(ask) => Changed::Removed(Cleared {
                outcome: worktree::remove(
                    &ask.checkout,
                    &ask.worktree,
                    &ask.branch,
                    &ask.build,
                    ask.remote.as_ref(),
                ),
                merged: ask.merged,
                friction: ask.friction,
            }),
        }
    }
}

impl Runner {
    /// Makes `change`, in a job when `off_lock` and a pass runs to apply it, else in place
    pub(in crate::runner) fn changing(
        &mut self,
        change: Change,
        off_lock: bool,
    ) -> Result<Begin, StateError> {
        if off_lock && self.snapshot.in_pass() {
            return Ok(self.change_job(change).map_or(Begin::Idle, Begin::Reads));
        }
        self.changed(change.run())
    }

    /// Applies what a change found to the work item in focus
    pub(in crate::runner) fn changed(&mut self, changed: Changed) -> Result<Begin, StateError> {
        match changed {
            Changed::Rebased(caught) => self.caught_up(caught),
            Changed::Removed(cleared) => self.cleaned_up(cleared),
        }
    }

    /// The rebase of the work item in focus, whose branch is at `head`, or none when it is gone
    pub(in crate::runner) fn rebasing(
        &self,
        number: u64,
        head: &str,
        review: Option<Review>,
    ) -> Option<Change> {
        let item = self.current()?;
        Some(Change::Rebase(Rebasing {
            checkout: self.settings.git.checkout.clone(),
            worktree: item.worktree.clone(),
            branch: item.branch.clone(),
            head: head.to_owned(),
            // An adopted branch's commits are someone else's, so it is merged, never rewritten.
            rewrite: !item.adopted,
            remote: item.remote.clone(),
            base: self.item_base(item).to_owned(),
            number,
            review,
        }))
    }
}
