//! A head the forge reports no check on
//!
//! With no workflow on `origin/main` that runs on a pull request, no check
//! will ever appear, so the head goes on as skipped. With one, a check may
//! still be starting, so the head waits `ci.queued_minutes` from the wait's
//! own `since`, and past it the item is parked on a `no-checks` ruling. A yes
//! to it passes that one head as skipped.

use crate::ports::Timestamp;
use crate::runner::Runner;
use crate::runner::report::Begin;
use crate::state::{StateError, Stuck};
use crate::work_item::Review;
use crate::worktree;

impl Runner {
    /// Whether `head` goes on with no check: a yes passed it as skipped, or
    /// `origin/main` holds no workflow that runs on a pull request
    pub(in crate::runner) fn expects_no_checks(&self, head: &str) -> Result<bool, Box<Begin>> {
        let Some(item) = self.current() else {
            return Err(Box::new(Begin::Idle));
        };
        if item.unchecked.as_deref() == Some(head) {
            return Ok(true);
        }
        let base = self.item_base(item);
        match worktree::runs_on_pull_requests(&self.settings.git.checkout, base) {
            Ok(runs) => Ok(!runs),
            Err(e) => {
                let reason = format!("cannot read the workflows on origin/{base}: {e}");
                Err(Box::new(self.gate_failed(reason)))
            }
        }
    }

    /// What the step does about `head`, which the forge reports no check on
    /// since `since`: `None` when it goes on as skipped, and the step's
    /// answer while it waits or once it is parked
    ///
    /// `review` is the round whose CI wait it is, when one.
    pub(in crate::runner) fn without_checks(
        &mut self,
        number: u64,
        head: &str,
        since: Timestamp,
        review: Option<&Review>,
    ) -> Result<Option<Begin>, StateError> {
        match self.expects_no_checks(head) {
            Ok(true) => return Ok(None),
            Ok(false) => {}
            Err(begin) => return Ok(Some(*begin)),
        }
        let cap = u64::from(self.settings.ci.queued_minutes.get()) * 60;
        let now = self.ports.clock.now();
        if now.0.saturating_sub(since.0) < cap {
            return Ok(Some(Begin::Idle));
        }
        let stuck = Stuck::NoChecks {
            head: head.to_owned(),
            review: review.cloned(),
        };
        self.raise(number, stuck.into()).map(Some)
    }
}
