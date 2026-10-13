//! How often a pass reads the forge
//!
//! The loop wakes for every call's news and every trigger that asks for
//! work, so a pass is no measure of time. The board is read at most once a
//! [`BOARD_POLL`], and at once after a change to what it would see: a work
//! item opening or ending, a ruling leaving the list, the adopted pull
//! requests queued changing, `finishing` ending, the project manager
//! answering, the issue writer's call ending, and the board's briefing
//! reading paths it had not compared. A step that fails waits [`RETRY_FIRST`] before it runs again,
//! twice as long after each failure in a row up to [`RETRY_MAX`], and no
//! less than the forge's rate-limit hold. Any other outcome ends the wait.
//! A write of kelpie's own, a comment, a label, a merge, makes the next look
//! due at once, since the read after it is the one that shows what it did.

use std::collections::BTreeMap;
use std::time::Duration;

use super::Runner;
use super::alert::backoff;
use super::report::{Begin, StepReport};
use super::turn::Slot;
use crate::ports::Timestamp;
use crate::state::ProjectState;

/// How often the board is read, however often the loop wakes
///
/// A read is two `gh` calls, 120 an hour, against GitHub's 5,000 an hour
/// for the maintainer's login. The open pull requests it lists are the
/// pass's snapshot, which the rework check, the CI gate and the merge read
/// instead of asking for each pull request.
pub const BOARD_POLL: Duration = Duration::from_secs(60);

/// How often the latest runs of `main`'s workflows are read, however often the
/// loop wakes
///
/// A read is one `gh` call, four an hour, and a red `main` does not change
/// faster than a CI run takes.
pub const MAIN_CI_POLL: Duration = Duration::from_secs(15 * 60);

/// Seconds a failed step waits before it runs again
const RETRY_FIRST: u64 = 15;

/// The longest a step that keeps failing waits, in seconds
const RETRY_MAX: u64 = 10 * 60;

/// The least the loop waits for a step that is due, so a step a pass did
/// not reach can never make the loop spin
const DUE_FLOOR: Duration = Duration::from_secs(1);

/// When the board was last read and each failing step runs again, in memory only
#[derive(Debug, Default)]
pub(super) struct Looks {
    board_read: Option<Timestamp>,
    // Whether something changed what the board would see since its last read
    board_moved: bool,
    // When the labels of dropped issues were last read on their own
    dropped_read: Option<Timestamp>,
    // When the contributions were last followed
    contributions_read: Option<Timestamp>,
    // Each failing step by its slot: its failures in a row, and when it runs again
    retries: BTreeMap<Slot, (u32, Timestamp)>,
    // How many writes of kelpie's the forge had taken when the last pass began
    writes_seen: u64,
    // When the workflows on `main` are read next, and the reads that failed in a row
    main_ci: Option<(u32, Timestamp)>,
}

impl Looks {
    /// Whether the labels of dropped issues are due a read at `now`, which
    /// records the read: at most one in each [`BOARD_POLL`], and none in one
    /// where the board itself was read, since that reads them
    pub(super) fn dropped_due(&mut self, now: Timestamp) -> bool {
        let poll = BOARD_POLL.as_secs();
        let recent = |at: Option<Timestamp>| at.is_some_and(|at| now.0 < at.0.saturating_add(poll));
        if recent(self.dropped_read) || recent(self.board_read) {
            return false;
        }
        self.dropped_read = Some(now);
        true
    }

    /// Whether `main`'s workflows are due a read at `now`
    pub(super) fn main_ci_due(&self, now: Timestamp) -> bool {
        self.main_ci.is_none_or(|(_, at)| now >= at)
    }

    /// Records that `main`'s workflows were read at `now`, which `read` says
    /// went well or not, with the forge held until `held`: the next read is
    /// a [`MAIN_CI_POLL`] on, or after a failure the wait any failed step has
    pub(super) fn main_ci_read(&mut self, now: Timestamp, read: bool, held: Option<Timestamp>) {
        let failures = self.main_ci.map_or(0, |(n, _)| n);
        self.main_ci = Some(if read {
            (0, Timestamp(now.0.saturating_add(MAIN_CI_POLL.as_secs())))
        } else {
            let wait = backoff(RETRY_FIRST, failures + 1, RETRY_MAX);
            let at = Timestamp(now.0.saturating_add(wait)).max(held.unwrap_or(now));
            (failures + 1, at)
        });
    }

    /// Whether the contributions are due to be followed at `now`, which
    /// records the read: at most one in each [`BOARD_POLL`]
    pub(super) fn contributions_due(&mut self, now: Timestamp) -> bool {
        if self.contributions_wait(now).is_some() {
            return false;
        }
        self.contributions_read = Some(now);
        true
    }

    /// How long until the contributions are next followed, or `None` when
    /// they are due now
    pub(super) fn contributions_wait(&self, now: Timestamp) -> Option<Duration> {
        let at = self
            .contributions_read?
            .0
            .saturating_add(BOARD_POLL.as_secs());
        (now.0 < at).then(|| Duration::from_secs(at - now.0))
    }

    /// Whether `slot`'s step may run at `now`
    pub(super) fn due(&self, slot: Slot, now: Timestamp) -> bool {
        let waited = (self.retries.get(&slot)).is_none_or(|&(_, at)| now >= at);
        let looked = match slot {
            Slot::Board => self.board_moved || self.board_due_at().is_none_or(|at| now >= at),
            Slot::Item(_) | Slot::Contributions => true,
        };
        waited && looked
    }

    /// Records that the board is read at `now`, before the read, so a
    /// change the read itself makes lets the next pass read it again
    pub(super) fn reading_board(&mut self, now: Timestamp) {
        self.board_read = Some(now);
        self.board_moved = false;
    }

    /// Records what `slot`'s step began at `now`, with the forge held until `held`
    pub(super) fn stepped(
        &mut self,
        slot: Slot,
        begin: &Begin,
        now: Timestamp,
        held: Option<Timestamp>,
    ) {
        let failed = matches!(
            begin,
            Begin::Report(
                StepReport::GateFailed { .. }
                    | StepReport::BoardFailed { .. }
                    | StepReport::ParentCloseFailed { .. }
            )
        );
        if !failed {
            self.retries.remove(&slot);
            return;
        }
        let failures = self.retries.get(&slot).map_or(0, |&(n, _)| n) + 1;
        let wait = backoff(RETRY_FIRST, failures, RETRY_MAX);
        let at = Timestamp(now.0.saturating_add(wait)).max(held.unwrap_or(now));
        self.retries.insert(slot, (failures, at));
    }

    /// Records that a pass begins with `writes` of kelpie's made so far
    pub(super) fn pass_began(&mut self, writes: u64) {
        self.writes_seen = writes;
    }

    /// Whether `writes` are more than the last pass began with, so what
    /// kelpie wrote has not been read yet
    pub(super) fn written_since(&self, writes: u64) -> bool {
        writes > self.writes_seen
    }

    /// Lets the board be read on the next pass
    pub(super) fn board_moved(&mut self) {
        self.board_moved = true;
    }

    /// Forgets the waits of work items no pass runs now: one that ended or
    /// has a call in flight
    ///
    /// The board's wait stays while it opens nothing, since a slot taken
    /// and freed again must not let a failed read run before its backoff.
    pub(super) fn keep_only(&mut self, open: &[Slot]) {
        self.retries
            .retain(|slot, _| *slot == Slot::Board || open.contains(slot));
    }

    /// How long until the soonest of the `open` steps is due: the board's
    /// next read, or a failed step's retry, or `None` with neither waiting
    ///
    /// A step already due is the pass's to run, so the wait is never under
    /// [`DUE_FLOOR`], and a step not in `open` sets none.
    pub(super) fn next_due(&self, now: Timestamp, open: &[Slot]) -> Option<Duration> {
        let due_at = |slot: &Slot| {
            let retry = self.retries.get(slot).map(|&(_, at)| at);
            match slot {
                Slot::Item(_) | Slot::Contributions => retry,
                Slot::Board => {
                    let read = match self.board_due_at() {
                        Some(at) if !self.board_moved => at,
                        _ => now,
                    };
                    Some(retry.map_or(read, |retry| retry.max(read)))
                }
            }
        };
        let soonest = open.iter().filter_map(due_at).min()?;
        let wait = Duration::from_secs(soonest.0.saturating_sub(now.0));
        Some(wait.max(DUE_FLOOR))
    }

    fn board_due_at(&self) -> Option<Timestamp> {
        let poll = BOARD_POLL.as_secs();
        (self.board_read).map(|at| Timestamp(at.0.saturating_add(poll)))
    }
}

/// Whether a save from `before` to `after` changes what the board would see
pub(super) fn board_moved(before: &ProjectState, after: &ProjectState) -> bool {
    let issues = |s: &ProjectState| s.work_items.iter().map(|i| i.issue).collect::<Vec<_>>();
    let ruled = (before.rulings.iter()).any(|r| !after.rulings.iter().any(|a| a.id == r.id));
    issues(before) != issues(after)
        || ruled
        || before.adopted != after.adopted
        || (before.finishing && !after.finishing)
}

impl Runner {
    /// How long until the runner next reads the board, retries a failed
    /// step or tries a turn again whose model host could not be reached, or
    /// `None` with none waiting
    pub fn next_look(&self) -> Option<Duration> {
        let now = self.ports.clock.now();
        let looks = self.looks.next_due(now, &self.rotation());
        let host = self.next_retry().map(|d| d.max(DUE_FLOOR));
        let wrote = (self.looks.written_since(self.forge_ledger.writes())).then_some(DUE_FLOOR);
        let followed = (!self.settings.contributions.is_off()).then(|| {
            self.looks
                .contributions_wait(now)
                .unwrap_or_default()
                .max(DUE_FLOOR)
        });
        [looks, host, wrote, followed].into_iter().flatten().min()
    }
}

#[cfg(test)]
mod tests;
