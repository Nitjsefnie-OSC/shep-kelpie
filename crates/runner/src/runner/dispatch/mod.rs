//! Dispatch: the board's next issue opens a work item in a free slot
//!
//! A running project with fewer than `concurrency.active_items` working and fewer than
//! `concurrency.pending_rulings` parked asks the board on its steps, after starting any pull
//! request it adopted and checking its open pull requests for one asking
//! for a rework, and before a call a contribution is due goes ahead of any
//! issue, and the research on an untouched issue after that. A queued issue
//! waits until a slot is free, an issue, or a pull request, whose work item is
//! open waits for it to end, and an issue naming a file a parked item's
//! branch changes waits for that item. With a project manager and two or more
//! issues the board could take, its pick goes in place of the board rule's,
//! and a ready issue it holds waits.

use std::sync::{Arc, Mutex};

use super::Runner;
use super::pm::Choice;
use super::report::{Begin, StepReport};
use super::trigger::lock;
use crate::board::{self, ReadyIssue, Skip};
use crate::ports::{Issue, Tickets};
use crate::settings::ForgeSlug;
use crate::state::StateError;
use crate::ticket::TicketId;

// The board is read only while it may open a work item, so a dropped ticket's label is read on
// its own when it has not been, at most once a poll, with the runner's lock let go. One that is
// closed or lost `ready-for-agent` is forgotten, so putting the label back returns it. One the
// forge cannot answer for stays, and is read again at the next pass.
pub(super) fn recheck_dropped(runner: &Mutex<Runner>) -> Result<(), StateError> {
    let job = lock(runner).dropped_job();
    let Some(job) = job else {
        return Ok(());
    };
    let read = job.run();
    lock(runner).apply_dropped(read)
}

// The dropped tickets whose labels are due a read
struct DroppedJob {
    tickets: Arc<dyn Tickets>,
    repo: ForgeSlug,
    dropped: Vec<TicketId>,
}

// What the read found: the tickets no longer labelled ready, and whether every one was read
struct DroppedRead {
    gone: Vec<TicketId>,
    complete: bool,
}

impl DroppedJob {
    fn run(self) -> DroppedRead {
        let unlabelled =
            |issue: &Issue| !issue.open || !issue.labels.iter().any(|l| l == board::READY);
        let mut read = DroppedRead {
            gone: Vec::new(),
            complete: true,
        };
        for ticket in self.dropped {
            match self.tickets.issue(&self.repo, ticket) {
                Ok(issue) if unlabelled(&issue) => read.gone.push(ticket),
                Ok(_) => {}
                Err(_) => read.complete = false,
            }
        }
        read
    }
}

impl Runner {
    // The read, when a dropped ticket is due one
    fn dropped_job(&self) -> Option<DroppedJob> {
        if self.state.dropped.is_empty() || !self.looks.dropped_due(self.ports.clock.now()) {
            return None;
        }
        Some(DroppedJob {
            tickets: Arc::clone(&self.ports.tickets),
            repo: self.remote.clone(),
            dropped: self.state.dropped.clone(),
        })
    }

    // Forgets the tickets the read found unlabelled that are still dropped, and records the read
    // unless a ticket could not be read
    fn apply_dropped(&mut self, read: DroppedRead) -> Result<(), StateError> {
        let gone: Vec<TicketId> = (read.gone.into_iter())
            .filter(|t| self.state.dropped.contains(t))
            .collect();
        if !gone.is_empty() {
            let mut next = self.state.clone();
            next.dropped.retain(|n| !gone.contains(n));
            self.save(next)?;
        }
        if read.complete {
            self.looks.dropped_read(self.ports.clock.now());
        }
        Ok(())
    }

    // The step runs this only while the board may open a work item, and it
    // opens at most one, so nothing here checks `concurrency.active_items` again.
    pub(super) fn dispatch(&mut self) -> Result<Begin, StateError> {
        // The board was read before the pass took the lock, unless it opened since.
        let read = self.take_board_read();
        let listed = read
            .ready
            .and_then(|ready| self.open_listed().map(|open| (ready, open)));
        let (mut ready, open) = match listed {
            Ok(listed) => listed,
            Err(e) => {
                return Ok(Begin::Report(StepReport::BoardFailed {
                    reason: format!("cannot read the board: {e}"),
                }));
            }
        };
        // A read that failed leaves no stamp, so it waits its backoff and no poll. Stamped before
        // anything the board does, so a change that does lets the next pass read again.
        self.looks.reading_board(self.ports.clock.now());
        self.follow_order(&ready)?;
        self.trim_finished(&read.closed)?;
        self.trim_dropped(&ready)?;
        // An issue in flight is left out, and not listed in `skipped`: it is
        // being worked on, not passed over.
        ready.retain(|issue| self.state.item(issue.ticket).is_none());
        self.ready_read(&ready)?;
        if let Some(begin) = self.close_done_parent(&ready, read.unlisted) {
            return Ok(begin);
        }
        // An adopted pull request, then one asking for a rework, goes before
        // any ready issue, and one that cannot start is passed over like one.
        let (begin, mut failed) = self.adopt_waiting(&open)?;
        if let Some(begin) = begin {
            self.set_failed_skips(failed);
            return Ok(begin);
        }
        let (begin, reworks) = self.rework_asked(&open)?;
        failed.extend(reworks);
        if let Some(begin) = begin {
            self.set_failed_skips(failed);
            return Ok(begin);
        }
        // A contribution's call goes before any new issue, and takes the slot
        // the board would have.
        if let Some(begin) = self.contribution_due()? {
            self.set_failed_skips(failed);
            return Ok(begin);
        }
        // An untouched issue's research goes after that and before the board's
        // own pick, and takes the slot it would have.
        if let Some(begin) = self.triage_due()? {
            self.set_failed_skips(failed);
            return Ok(begin);
        }
        // One whose paths a parked item's branch touches would build on code
        // that may yet change, and one whose paths are unread may be one.
        let overlaps = self.parked_overlap(&ready);
        ready.retain(|issue| !overlaps.iter().any(|s| s.issue() == Some(issue.ticket)));
        let implementers = self.agents.implementer_names();
        let all = takeable(
            &ready,
            &open,
            &self.state.finished,
            &self.state.dropped,
            &implementers,
        );
        let held = self.pm_holds(&all);
        ready.retain(|issue| !held.contains(&issue.ticket));
        let mut paced = false;
        let mut unlabelled: Vec<Skip> = Vec::new();
        loop {
            let implementers = self.agents.implementer_names();
            let (finished, dropped) = (self.state.finished.clone(), self.state.dropped.clone());
            let order = self.state.order.clone();
            let pick = board::pick(&ready, &open, &finished, &dropped, &implementers, &order);
            let mut skipped = pick.skipped;
            skipped.extend(failed.iter().cloned());
            skipped.extend(overlaps.iter().cloned());
            skipped.extend(unlabelled.iter().cloned());
            skipped.sort_by_key(Skip::issue);
            let takeable = takeable(&ready, &open, &finished, &dropped, &implementers);
            let Some(picked) = pick.issue else {
                let reason = failed
                    .iter()
                    .filter_map(|skip| match skip {
                        Skip::Failed { issue, error } => {
                            Some(format!("cannot dispatch #{issue}: {error}"))
                        }
                        Skip::Rework {
                            pull_request,
                            error,
                            ..
                        } => Some(format!("cannot rework #{pull_request}: {error}")),
                        Skip::Adopt {
                            pull_request,
                            error,
                            ..
                        } => Some(format!("cannot adopt #{pull_request}: {error}")),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                self.set_skipped(skipped);
                return Ok(if failed.is_empty() {
                    Begin::Idle
                } else {
                    Begin::Report(StepReport::BoardFailed { reason })
                });
            };
            if !paced {
                if let Some(held) = self.pace_dispatch()?.holds() {
                    self.set_skipped(skipped);
                    return Ok(held);
                }
                paced = true;
            }
            let ticket = match self.pm_pick(&takeable) {
                Choice::Take(n) => n,
                Choice::Rule => picked,
                Choice::Wait => {
                    self.set_skipped(skipped);
                    return Ok(Begin::Idle);
                }
            };
            // An issue the issue writer is labelling waits for it, even one
            // labelled by hand meanwhile. A local ticket has no issue writer's
            // label: its `agent:` label, or the project's only implementer, sets its agent.
            let labelling = self.flights.labelling() == Some(ticket);
            if let Some(found) = ready.iter().find(|i| i.ticket == ticket)
                && ticket.github().is_some()
                && (labelling || self.needs_label(found))
            {
                let ruling = self.unlabelled_ruling(ticket);
                if labelling
                    || ruling.is_some()
                    || self.draining()
                    || self.flights.labelling().is_some()
                {
                    unlabelled.push(Skip::Unlabelled {
                        issue: ticket,
                        ruling,
                    });
                    ready.retain(|i| i.ticket != ticket);
                    continue;
                }
                if let Some(held) = self.pace_writer()?.holds() {
                    self.set_skipped(skipped);
                    return Ok(held);
                }
                let found = found.clone();
                self.set_skipped(skipped);
                return Ok(Begin::Label(found));
            }
            match self.add(ticket) {
                Ok(agent) => {
                    self.set_skipped(skipped.clone());
                    return Ok(Begin::Report(StepReport::Dispatched {
                        issue: ticket,
                        agent,
                        skipped,
                    }));
                }
                Err(e @ (super::AddError::Forge(_) | super::AddError::Label(_))) => {
                    failed.push(Skip::Failed {
                        issue: ticket,
                        error: e.to_string(),
                    });
                    ready.retain(|i| i.ticket != ticket);
                }
                Err(super::AddError::State(e)) => return Err(e),
                Err(e) => {
                    self.set_skipped(skipped);
                    return Ok(Begin::Report(StepReport::BoardFailed {
                        reason: format!("cannot dispatch {}: {e}", ticket.shown()),
                    }));
                }
            }
        }
    }
}

// The ready issues the board could take now, each on its own.
fn takeable(
    ready: &[ReadyIssue],
    open: &[board::OpenPullRequest],
    finished: &[TicketId],
    dropped: &[TicketId],
    implementers: &[crate::settings::AgentName],
) -> Vec<TicketId> {
    (ready.iter())
        .filter(|i| {
            let one = std::slice::from_ref(*i);
            board::pick(one, open, finished, dropped, implementers, &[])
                .issue
                .is_some()
        })
        .map(|i| i.ticket)
        .collect()
}

impl Runner {
    // Each read of the board updates the order it takes the ready issues in:
    // a new issue joins it, one whose priority label changed moves, and one
    // the forge stopped listing leaves it.
    fn follow_order(&mut self, ready: &[ReadyIssue]) -> Result<(), StateError> {
        let (order, ranked) = board::keep_order(&self.state.order, &self.state.ranked, ready);
        if order == self.state.order && ranked == self.state.ranked {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.order = order;
        next.ranked = ranked;
        self.save(next)
    }

    // The board skips a finished issue until the forge closes it, so an entry
    // goes only once its ticket reads as closed. One the forge lists ready is
    // open, and one it cannot answer for stays. The read names the `closed` ones.
    fn trim_finished(&mut self, closed: &[TicketId]) -> Result<(), StateError> {
        if closed.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.finished.retain(|n| !closed.contains(n));
        self.save(next)
    }

    // The board leaves a dropped ticket be while the forge lists it ready, so
    // an entry goes once the forge stops listing it: a label put back is then
    // new.
    fn trim_dropped(&mut self, ready: &[ReadyIssue]) -> Result<(), StateError> {
        let kept: Vec<TicketId> = (self.state.dropped.iter())
            .filter(|t| ready.iter().any(|r| &r.ticket == *t))
            .copied()
            .collect();
        if kept.len() == self.state.dropped.len() {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.dropped = kept;
        self.save(next)
    }
}

#[cfg(test)]
mod tests;
