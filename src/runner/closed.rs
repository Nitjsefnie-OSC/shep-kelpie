//! Work items whose issue was closed with no change
//!
//! A work item with no pull request ends once its issue is closed while its
//! branch and worktree hold no work: a turn that ends so finishes it with no
//! nudge and no ruling, and a pass ends a parked one and withdraws its
//! ruling. Kelpie cannot tell who closed the issue. Work found, or a git
//! that cannot say, leaves the item to go on as before.
//!
//! A parked work item with a pull request reaches no gate to notice it
//! merged by hand, so a pass ends it the same way. One closed without
//! merging gets the gate's `closed` ruling in place of its old one.

use super::Runner;
use super::ruling::park;
use crate::ports::{Alert, PullRequestState};
use crate::state::{ProjectState, RulingKind, StateError, Stuck};
use crate::ticket::TicketId;
use crate::work_item::{Phase, WorkItem};
use crate::worktree;

/// Seconds between reads of a parked work item's issue. A parked item waits
/// hours, and each read is a forge call and a fetch.
pub(super) const PARKED_READ: u64 = 5 * 60;

impl Runner {
    /// Whether `ticket`'s work item has its ticket closed and no work on its
    /// branch or in its worktree. A git that cannot say is noted, and counts
    /// as work.
    ///
    /// # Errors
    ///
    /// Why the forge could not be asked.
    pub(super) fn closed_with_no_change(&mut self, ticket: TicketId) -> Result<bool, String> {
        let item = self.state.item(ticket).expect("an open work item");
        let (branch, tree) = (item.branch.clone(), item.worktree.clone());
        let read = self.ports.tickets.issue(&self.remote, ticket);
        if read
            .map_err(|e| format!("cannot read {}: {e}", ticket.named().to_lowercase()))?
            .open
        {
            return Ok(false);
        }
        match worktree::holds_work(&self.settings.git.checkout, &tree, &branch) {
            Ok(held) => Ok(!held),
            Err(e) => {
                let note = format!(
                    "{} is closed, but its work cannot be read: {e}",
                    ticket.named().to_lowercase()
                );
                self.notes.push(note);
                Ok(false)
            }
        }
    }

    /// Ends each parked work item whose issue was closed with no change, or
    /// whose pull request was merged, withdrawing its ruling. A parked item
    /// whose pull request was closed without merging gets a `stuck` ruling
    /// for `closed` in place of its old one.
    ///
    /// Each such item is read at most once every [`PARKED_READ`] seconds,
    /// counted in memory, so a restart reads it at once: its pull request
    /// when it has one, else its issue. An open pull request that closes the
    /// issue keeps an item with none. An attached item is left alone, and so
    /// is one parked on its follow-up ruling, whose merge kelpie made. One
    /// parked on `closed` is read on, but only a merge changes it, so a pull
    /// request reopened and merged still ends the item. A forge that cannot
    /// be asked is noted and asked again at the next read.
    ///
    /// # Errors
    ///
    /// [`StateError::Write`] when the change cannot be saved.
    pub(super) fn read_parked(&mut self) -> Result<(), StateError> {
        let now = self.ports.clock.now();
        let read = &self.parked_reads;
        let due = |ticket: TicketId| {
            read.get(&ticket)
                .is_none_or(|at| now.0.saturating_sub(at.0) >= PARKED_READ)
        };
        let parked = self.issues_where(|i| {
            i.parked()
                && i.attached.is_none()
                && due(i.issue)
                && !self.parked_on(i, |k| matches!(k, RulingKind::FollowUp { .. }))
        });
        let mut ended = Vec::new();
        let mut found = Vec::new();
        for &ticket in &parked {
            self.parked_reads.insert(ticket, now);
            let item = self.state.item(ticket).expect("an open work item");
            if let Some(number) = item.pull_request {
                match self.ports.forge.pull_request(&self.remote, number) {
                    Ok(pr) if pr.state == PullRequestState::Open => {}
                    // Found closed already, the ruling stands. A merge after
                    // a reopen still ends the item.
                    Ok(pr)
                        if pr.state == PullRequestState::Closed
                            && self.parked_on(item, |k| {
                                matches!(k, RulingKind::Stuck(Stuck::Closed))
                            }) => {}
                    Ok(pr) => found.push((ticket, number, pr.state)),
                    Err(e) => self.notes.push(format!("cannot read #{number}: {e}")),
                }
                continue;
            }
            match self.closed_with_no_change(ticket) {
                Ok(true) => ended.push(ticket),
                Ok(false) => {}
                Err(why) => self.notes.push(why),
            }
        }
        if !ended.is_empty() {
            // A pull request on another branch that closes the issue is work too.
            match self.ports.forge.open_pull_requests(&self.remote) {
                Ok(open) => ended.retain(|&ticket| {
                    // A local ticket is closed by no GitHub pull request.
                    let Some(number) = ticket.github() else {
                        return true;
                    };
                    !open.iter().any(|pr| pr.closes.contains(&number))
                }),
                Err(e) => {
                    let note = format!("cannot list open pull requests: {e}");
                    self.notes.push(note);
                    ended.clear();
                }
            }
        }
        if ended.is_empty() && found.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        let withdrawn: Vec<(TicketId, Option<TicketId>)> = (ended.iter())
            .map(|&ticket| (ticket, end_closed(&mut next, ticket)))
            .collect();
        let mut merged = Vec::new();
        let mut unmerged = Vec::new();
        for (ticket, number, state) in found {
            if state == PullRequestState::Merged {
                merged.push((ticket, number, end_merged(&mut next, ticket)));
            } else {
                next.rulings.retain(|r| r.issue != Some(ticket));
                let kind = Stuck::Closed.into();
                let (id, _) = park(self.names(), &mut next, ticket, Some(number), kind);
                unmerged.push((number, id));
            }
        }
        if let Err(e) = self.save(next) {
            // Read again on the next pass, before any answer resumes a worker.
            for ticket in &parked {
                self.parked_reads.remove(ticket);
            }
            return Err(e);
        }
        for (ticket, id) in withdrawn {
            self.say_closed(ticket, id);
        }
        for (ticket, number, id) in merged {
            self.say_merged(ticket, number, id);
        }
        for (number, id) in unmerged {
            let note =
                format!("pull request #{number} was closed without merging, so ruling {id} asks");
            self.notes.push(note);
            if let Some(why) = self.post_ruling(Some(number), id) {
                let note = format!("cannot post ruling {id} on its thread: {why}");
                self.notes.push(note);
            }
        }
        Ok(())
    }

    // Whether `item` is parked on a ruling that `kind` matches
    fn parked_on(&self, item: &WorkItem, kind: impl Fn(&RulingKind) -> bool) -> bool {
        let Phase::Ruling { id } = item.phase else {
            return false;
        };
        (self.state.rulings.iter())
            .find(|r| r.id == id)
            .is_some_and(|r| kind(&r.kind))
    }

    /// Logs that pull request `number` was merged, so the work item for
    /// `ticket` ends, and that ruling `id` is withdrawn, if it was parked on
    /// one. Posts it once to the webhook, where one is set: nothing is left
    /// to retry it for.
    fn say_merged(&mut self, ticket: TicketId, number: u64, id: Option<TicketId>) {
        let project = self.project.as_str().to_owned();
        let withdrawn = id.map_or_else(String::new, |id| format!(" Ruling {id} is withdrawn."));
        let text = format!(
            "Pull request #{number} was merged, so work item {} ends.{withdrawn} \
             Nothing to answer.",
            ticket.shown()
        );
        self.notes.push(text.clone());
        let Some(webhook) = self.webhook.clone() else {
            return;
        };
        let alert = Alert {
            title: format!("kelpie: {project} {} merged", ticket.shown()),
            text,
            reply: None,
        };
        if let Err(e) = self.ports.alerts.post(&webhook, &alert) {
            let failed = format!("cannot post that #{number} was merged: {e}");
            self.notes.push(failed);
        }
    }

    /// Logs that the ticket `ticket` was closed with no change, so its work
    /// item ends, and that ruling `id` is withdrawn, if it was parked on one.
    /// Posts it once to the webhook, where one is set: nothing is left to
    /// retry it for.
    pub(super) fn say_closed(&mut self, ticket: TicketId, id: Option<TicketId>) {
        let project = self.project.as_str().to_owned();
        let withdrawn = id.map_or_else(String::new, |id| format!(" Ruling {id} is withdrawn."));
        let text = format!(
            "{} was closed with no change, so its work item ends with no pull \
             request.{withdrawn} Nothing to answer.",
            ticket.named()
        );
        self.notes.push(text.clone());
        // A local ticket has no forge thread to post the notice on.
        if let Some(number) = ticket.github() {
            self.notice_on(number, &text);
        }
        let Some(webhook) = self.webhook.clone() else {
            return;
        };
        let alert = Alert {
            title: format!("kelpie: {project} {} closed with no change", ticket.shown()),
            text,
            reply: None,
        };
        if let Err(e) = self.ports.alerts.post(&webhook, &alert) {
            let failed = format!(
                "cannot post that {} was closed with no change: {e}",
                ticket.shown()
            );
            self.notes.push(failed);
        }
    }
}

/// Marks `issue`'s work item in `next` to end, its pull request merged, and
/// drops its rulings. Returns the ruling it was parked on, if any.
fn end_merged(next: &mut ProjectState, ticket: TicketId) -> Option<TicketId> {
    let item = next
        .item_mut(ticket)
        .expect("a work item that ends is open");
    let id = match item.phase {
        Phase::Ruling { id } => Some(id),
        _ => None,
    };
    item.phase = Phase::Done {
        merged: true,
        closed: false,
    };
    next.rulings.retain(|r| r.issue != Some(ticket));
    id
}

/// Marks `issue`'s work item in `next` to end, closed with no change, and
/// drops its rulings. Returns the ruling it was parked on, if any.
pub(super) fn end_closed(next: &mut ProjectState, ticket: TicketId) -> Option<TicketId> {
    let item = next
        .item_mut(ticket)
        .expect("a work item that ends is open");
    let id = match item.phase {
        Phase::Ruling { id } => Some(id),
        _ => None,
    };
    item.phase = Phase::Done {
        merged: false,
        closed: true,
    };
    next.rulings.retain(|r| r.issue != Some(ticket));
    id
}

#[cfg(test)]
mod tests;
