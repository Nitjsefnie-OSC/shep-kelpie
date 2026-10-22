//! Slots: how many work items call models at once, and how many wait on rulings
//!
//! A slot bounds model calls: a worker's turn or a review. A work item
//! takes one when it opens, and keeps it through CI and the merge. Parked
//! on a ruling it gives the slot up: a parked worker runs no process, so an
//! unanswered ruling never stalls the project. A `Done` item gives it up
//! too, since its cleanup calls no model and may keep failing.
//! `concurrency.pending_rulings` caps the items parked, and while that many
//! wait the board opens nothing new. Answered, an item goes on without a
//! slot through anything that calls no model (CI, a merge, its end), and
//! waits for one, ahead of any new work item, once it needs a turn or a
//! review.
//!
//! A contribution holds no slot while it waits on its contributor. A model
//! call on it takes one for as long as it runs and gives it back after, and
//! it starts only where one is free, ahead of any new work item. An item
//! waiting for a slot is seated before such a call starts, and not while one
//! runs.

use super::Runner;
use crate::board::briefing::{Files, shared};
use crate::board::{ReadyIssue, Skip};
use crate::state::{ProjectState, RulingKind, StateError};
use crate::ticket::TicketId;
use crate::work_item::{Phase, Seat, WorkItem};

/// Seats the work items in `next`, then gives the slots free under `max`
/// to the items waiting, oldest first
///
/// A parked or `Done` item holds no slot. One that left a ruling, or goes on
/// without a slot, waits for one once its phase calls a model, so no model
/// call runs past `max`. Returns whether any seat changed.
pub(super) fn seat(before: &ProjectState, next: &mut ProjectState, max: usize) -> bool {
    let was: Vec<Seat> = next.work_items.iter().map(|i| i.seat).collect();
    for item in &mut next.work_items {
        let left = !item.parked() && before.item(item.issue).is_some_and(WorkItem::parked);
        item.seat = match item.seat {
            _ if item.parked() || matches!(item.phase, Phase::Done { .. }) => Seat::Without,
            Seat::Held if !left => Seat::Held,
            _ if item.calls_a_model() => Seat::Waiting,
            _ => Seat::Without,
        };
    }
    let held = (next.work_items.iter())
        .filter(|i| i.seat == Seat::Held)
        .count();
    let free = max.saturating_sub(held);
    for item in (next.work_items.iter_mut())
        .filter(|i| i.seat == Seat::Waiting)
        .take(free)
    {
        item.seat = Seat::Held;
    }
    !next.work_items.iter().map(|i| i.seat).eq(was)
}

impl Runner {
    // The tickets of the open work items `keep` keeps, oldest first
    pub(super) fn issues_where(&self, keep: impl Fn(&WorkItem) -> bool) -> Vec<TicketId> {
        (self.state.work_items.iter())
            .filter(|i| keep(i))
            .map(|i| i.issue)
            .collect()
    }

    // The open work items that hold a slot or wait for one, then the
    // pull request of the contribution a call runs on, then the issue the
    // triager researches
    pub(super) fn slot_issues(&self) -> Vec<TicketId> {
        let mut held = self.issues_where(|i| !i.parked() && i.seat != Seat::Without);
        held.extend(
            self.flights
                .securing_pull_request()
                .map(TicketId::from_pull_request),
        );
        held.extend((self.flights.triaging()).and_then(TicketId::from_issue));
        held
    }

    // How many slots a call on a contribution or an issue under triage holds now
    fn contribution_slots(&self) -> usize {
        usize::from(self.flights.securing_pull_request().is_some())
            + usize::from(self.flights.triaging().is_some())
    }

    // The slots the work items share: `concurrency.active_items`, less those
    // that calls on contributions hold
    pub(super) fn item_slots(&self) -> usize {
        self.active_items()
            .saturating_sub(self.contribution_slots())
    }

    // Whether another work item may open, or a call on a contribution start,
    // under `concurrency.active_items`. An item waiting for a slot takes it first.
    pub(super) fn slot_free(&self) -> bool {
        self.slot_issues().len() < self.active_items()
    }

    // The open work items parked on rulings
    pub(super) fn parked_issues(&self) -> Vec<TicketId> {
        self.issues_where(WorkItem::parked)
    }

    // Whether `item` is parked on a ruling that holds its work back: any
    // but the follow-up ruling a merged pull request leaves
    fn held_back(&self, item: &WorkItem) -> bool {
        let Phase::Ruling { id } = item.phase else {
            return false;
        };
        let ruling = self.state.rulings.iter().find(|r| r.id == id);
        !ruling.is_some_and(|r| matches!(r.kind, RulingKind::FollowUp { .. }))
    }

    // How many work items `concurrency.pending_rulings` counts
    pub(super) fn parked_count(&self) -> usize {
        (self.state.work_items.iter())
            .filter(|i| self.held_back(i))
            .count()
    }

    // Whether `concurrency.pending_rulings` items wait on rulings, so the board opens nothing.
    // With none waiting it is never full, 0 included.
    pub(super) fn parked_full(&self) -> bool {
        let max = usize::try_from(self.settings.concurrency.pending_rulings).unwrap_or(usize::MAX);
        self.parked_count() >= max.max(1)
    }

    // Whether a slot is free for the work item for `issue`, which holds
    // none: one that no item holding a slot, or waiting ahead of it, takes
    pub(super) fn slot_open_for(&self, ticket: TicketId) -> bool {
        let waits = self
            .state
            .item(ticket)
            .is_some_and(|i| i.seat == Seat::Waiting);
        let mut ahead = 0;
        let mut passed = false;
        for item in &self.state.work_items {
            if item.issue == ticket {
                passed = true;
                continue;
            }
            match item.seat {
                Seat::Held if !item.parked() => ahead += 1,
                Seat::Waiting if !waits || !passed => ahead += 1,
                _ => {}
            }
        }
        ahead < self.item_slots()
    }

    // Whether the current work item waits for a slot, so no model call may
    // start for it, even on a step that just moved it into a phase that calls one
    pub(super) fn unseated(&self) -> bool {
        self.current().is_some_and(|i| i.seat == Seat::Waiting)
    }

    // Whether the board may open a work item on this pass
    pub(super) fn board_open(&self) -> bool {
        self.picks() && self.slot_free() && !self.parked_full()
    }

    // Whether a contribution waits for its head to be read
    pub(super) fn contribution_unread(&self) -> bool {
        (self.state.contributions.iter()).any(|c| !c.is_read())
    }

    // Gives a slot freed outside a save, as by a larger `concurrency.active_items`, to an
    // item waiting for one, and takes the slot of a Done item a state file saved holding one.
    pub(super) fn seat_waiting(&mut self) -> Result<(), StateError> {
        let to_seat = |i: &WorkItem| {
            i.seat == Seat::Waiting
                || (i.seat != Seat::Without && matches!(i.phase, Phase::Done { .. }))
        };
        if !self.state.work_items.iter().any(to_seat) {
            return Ok(());
        }
        let mut next = self.state.clone();
        if seat(&self.state, &mut next, self.item_slots()) {
            self.save(next)?;
        }
        Ok(())
    }

    pub(super) fn active_items(&self) -> usize {
        usize::try_from(self.settings.concurrency.active_items.get()).unwrap_or(usize::MAX)
    }

    // What the alert for ruling `id` adds while `concurrency.pending_rulings` items wait,
    // the item parked on it among them
    pub(super) fn parked_note(&self, id: TicketId) -> Option<String> {
        let parks = (self.state.work_items.iter())
            .any(|i| i.phase == Phase::Ruling { id } && self.held_back(i));
        if !parks || !self.parked_full() {
            return None;
        }
        let waiting = match self.parked_count() {
            1 => "1 ruling is waiting".to_owned(),
            n => format!("{n} rulings are waiting"),
        };
        Some(format!(
            "{waiting}, and `concurrency.pending_rulings` is {}, so no new work item opens until one is answered.",
            self.settings.concurrency.pending_rulings
        ))
    }

    // The ready issues passed over against parked items' branches: one
    // whose named paths share a file with one, by the overlap the board
    // shows, and one that waits a pass to be read: every one while a parked
    // branch's files are unknown, else, while a parked branch touches any
    // file, one whose paths the board has not read from its body as it now
    // stands. Both are read with the board's next write. A merged pull
    // request's branch changes nothing more.
    pub(super) fn parked_overlap(&self, ready: &[ReadyIssue]) -> Vec<Skip> {
        let parked: Vec<(TicketId, Files)> = (self.state.work_items.iter())
            .filter(|i| self.held_back(i))
            .map(|i| (i.issue, self.item_files(i.issue)))
            .collect();
        let unread = |issue: &ReadyIssue, unknown| Skip::PathsUnread {
            issue: issue.ticket,
            unknown,
        };
        if let Some((with, _)) = parked.iter().find(|(_, f)| matches!(f, Files::Unknown)) {
            return ready.iter().map(|i| unread(i, Some(*with))).collect();
        }
        let mut skips = Vec::new();
        if parked.iter().all(|(_, files)| files.list().is_empty()) {
            return skips;
        }
        for issue in ready {
            let Some(named) = self.brief_named(issue.ticket) else {
                skips.push(unread(issue, None));
                continue;
            };
            let found = parked.iter().find_map(|(with, files)| {
                let both = shared(named, files.list());
                (!both.is_empty()).then_some((*with, both))
            });
            if let Some((with, files)) = found {
                skips.push(Skip::Overlap {
                    issue: issue.ticket,
                    with,
                    files: files.into_iter().map(str::to_owned).collect(),
                });
            }
        }
        skips
    }
}
