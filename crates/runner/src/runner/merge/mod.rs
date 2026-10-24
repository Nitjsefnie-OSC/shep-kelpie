//! The merge a yes or `auto` allows, and the cleanup after it
//!
//! A merge takes only the head it was asked about, while CI on it is still
//! green and it still has the latest `main`. Otherwise kelpie withdraws it,
//! goes back to CI, and asks or merges again. Under `auto` a refused merge
//! takes that path too, and a second refusal raises a ruling. Every step is
//! saved before the next, so a runner restarted mid-merge picks up where it
//! stopped.

use std::fmt;

use super::Runner;
use super::claim::main_fetch_key;
use super::gate::{settled, short};
use super::report::{Begin, StepReport};
use super::retro::journal::Entry;
use super::trigger::WhichItem;
use crate::ports::{Checks, PullRequestState};
use crate::settings::Merging;
use crate::state::{Finished, MergeNotice, Notice, RulingKind, StateError, Stuck};
use crate::ticket::TicketId;
use crate::work_item::{Phase, Review, ReviewCallState, ReviewStage, Turn};
use crate::worktree::{self, Base, Remote};

/// Why `drop` was refused
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropError {
    /// The trigger named no work item, or one not open
    Which(WhichItem),
    /// The worker's turn is running, and the work item stays until it ends
    TurnRunning(TicketId),
    /// A review call is running, and the work item stays
    /// until it ends, since its result runs outside the runner's lock
    ReviewRunning(TicketId),
    /// A yes is being carried out, and the merge is not stopped halfway
    Merging(TicketId),
    /// The maintainer's `attach`, by this process, holds the work item
    Attached(TicketId, u32),
    /// Its worktree, branch or build folder could not be removed
    Cleanup(String),
    /// The change could not be saved
    State(StateError),
}

impl fmt::Display for DropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Which(e) => e.fmt(f),
            Self::TurnRunning(issue) => {
                write!(f, "the worker's turn on {} is running", issue.shown())
            }
            Self::ReviewRunning(issue) => {
                write!(f, "a review round on {} is running", issue.shown())
            }
            Self::Merging(issue) => {
                write!(f, "the work item for {} is merging", issue.shown())
            }
            Self::Attached(issue, pid) => write!(
                f,
                "the work item for {} is attached, by process {pid}: quit that claude \
                 session first, which lets it go",
                issue.shown()
            ),
            Self::Cleanup(reason) => f.write_str(reason),
            Self::State(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for DropError {}

impl Runner {
    /// Ends a work item without merging it, the one `ticket` names or the
    /// only one open
    ///
    /// Its worktree, local branch and build folder go, and its issue is
    /// recorded as finished. Its pull request and branch on the forge stay,
    /// the pull request labelled `ready-for-human`.
    /// It works while the runner drains too.
    ///
    /// # Errors
    ///
    /// [`DropError`] when nothing is in flight, a turn, a review round or a
    /// merge is under way, or the cleanup fails. The work item stays then.
    pub fn drop_work_item(&mut self, ticket: Option<TicketId>) -> Result<(), DropError> {
        self.choose(ticket).map_err(DropError::Which)?;
        let item = self.current().expect("the work item chosen");
        if matches!(item.turn, Turn::Running { .. }) {
            return Err(DropError::TurnRunning(item.issue));
        }
        if matches!(item.review_call, ReviewCallState::Running { .. }) {
            return Err(DropError::ReviewRunning(item.issue));
        }
        // Its worktree is the attached session's working folder.
        if let Some(attached) = item.attached.as_ref().filter(|a| super::attach::live(a)) {
            return Err(DropError::Attached(item.issue, attached.by.pid));
        }
        // A merged pull request parked on its follow-up ruling is past its
        // merge: dropping it would hand the merged pull request back as unmerged.
        let follow_up_parked = |id| {
            let ruling = self.state.rulings.iter().find(|r| r.id == id);
            ruling.is_some_and(|r| matches!(r.kind, RulingKind::FollowUp { .. }))
        };
        if matches!(
            item.phase,
            Phase::Merge { .. } | Phase::Done { merged: true, .. }
        ) || matches!(item.phase, Phase::Ruling { id } if follow_up_parked(id))
        {
            return Err(DropError::Merging(item.issue));
        }
        let summoning = |review: &Review| {
            matches!(
                review.stage,
                ReviewStage::Summon { .. } | ReviewStage::Summoned { .. }
            )
        };
        if matches!(&item.phase, Phase::Review(review) if summoning(review)) {
            self.leave_round();
        }
        match self.cleanup(false).map_err(DropError::State)? {
            Begin::Report(StepReport::GateFailed { reason, .. }) => Err(DropError::Cleanup(reason)),
            _ => Ok(()),
        }
    }

    // Marking the draft ready can start a fresh CI run on the same head, so
    // that pass ends there. A later pass, once the checks have had time to
    // register, merges only on a green run.
    pub(super) fn merge(&mut self) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let (issue, Some(number)) = (item.issue, item.pull_request) else {
            return Ok(Begin::Idle);
        };
        let Phase::Merge {
            head,
            readied,
            auto,
        } = item.phase.clone()
        else {
            return Ok(Begin::Idle);
        };
        let tried = item.merge_tried.clone();
        let repo = self.remote.clone();
        // Read with the runner's lock let go, and the step made again with the result
        let pr = match self.gate_view(number) {
            Ok(pr) => pr,
            Err(begin) => return Ok(*begin),
        };
        match pr.state {
            PullRequestState::Open => {}
            // Kelpie's own merge at this head, when a restart or a lost
            // answer hid it, still gets its notice under `auto`.
            PullRequestState::Merged => {
                let notice = (auto && pr.head == head) || tried == Some(pr.head.clone());
                return self.merged(issue, number, pr.head, notice);
            }
            PullRequestState::Closed => {
                self.update(|item| item.merge_queued = None)?;
                return self.raise(number, Stuck::Closed.into());
            }
        }
        // Queued, the pull request is tested on top of the ones ahead of it
        // while `main` moves under it, so none of the gates below apply.
        if let Some(queued) = item.merge_queued {
            return self.queued(number, head, queued);
        }
        // The gate asks again once the project is no longer `auto`.
        if auto && self.settings.git.merging != Merging::Auto {
            let reason = "`git.merging` is no longer `auto`".to_owned();
            return self.withdraw(issue, number, auto, reason);
        }
        let now = self.ports.clock.now();
        let ci = self.settings.ci.block;
        let settling = ci && readied.is_some_and(|at| !settled(at, now));
        if pr.head != head {
            let reason = format!("#{number} moved to {}", short(&pr.head));
            return self.withdraw(issue, number, auto, reason);
        }
        // A run that marking the draft ready started is waited for; before
        // that, anything but green withdraws the yes.
        let green = match pr.checks {
            _ if !ci => true,
            Checks::Passed | Checks::Skipped => true,
            Checks::None | Checks::Pending(_) if readied.is_some() => false,
            Checks::None | Checks::Pending(_) | Checks::Failed(_) => {
                let reason = format!("CI on #{number} is no longer green");
                return self.withdraw(issue, number, auto, reason);
            }
        };
        match self.base_of(&head) {
            Ok(Base::Current) => {}
            Ok(Base::Lagging) => return Ok(Begin::Idle),
            Ok(Base::Behind) => {
                let since = if auto {
                    "the gate passed"
                } else {
                    "the question"
                };
                let reason = format!("main moved since {since}");
                return self.withdraw(issue, number, auto, reason);
            }
            Err(reason) => return Ok(self.gate_failed(reason)),
        }
        if pr.draft {
            if let Err(e) = self.ports.forge.mark_ready(&repo, number) {
                return Ok(self.gate_failed(format!("cannot mark #{number} ready: {e}")));
            }
            let readied = Some(now);
            self.update(|item| {
                item.phase = Phase::Merge {
                    head,
                    readied,
                    auto,
                };
                item.known.ready = true;
            })?;
            return Ok(Begin::Report(StepReport::MarkedReady {
                issue,
                pull_request: number,
            }));
        }
        // The forge already shows it ready: either this pass just read that
        // back, or a restart landed between the call above and its update.
        // Either way it is kelpie's own doing, not a foreign change, and the
        // gate must not mistake it for one on its next look.
        if !item.known.ready {
            self.update(|item| item.known.ready = true)?;
        }
        if settling || !green {
            return Ok(Begin::Idle);
        }
        // A queued pull request is not merged yet and a second merge call
        // would refuse it, so a restart that lost the mark finds it here.
        let standing = match self.queue_standing(number) {
            Ok(standing) => standing,
            Err(begin) => return Ok(*begin),
        };
        if standing.queued || standing.armed {
            return self.queue_marked(standing.removals);
        }
        let removals = standing.removals;
        if let Err(e) = self.ports.forge.merge(&repo, number, &head) {
            let reason = format!("cannot merge #{number}: {e}");
            if !auto {
                return Ok(self.gate_failed(reason));
            }
            // A lost answer may have queued it rather than merged it.
            let waiting = self
                .ports
                .forge
                .merge_queue(&repo, number)
                .is_ok_and(|standing| standing.queued || standing.armed);
            if waiting {
                return self.queue_marked(removals);
            }
            match self.ports.forge.pull_request(&repo, number) {
                Ok(pr) if pr.state == PullRequestState::Merged && pr.head == head => {}
                Ok(_) => return self.refused(issue, number, head, reason),
                Err(e) => return Ok(self.gate_failed(format!("cannot read #{number}: {e}"))),
            }
        }
        // With a merge queue on, the call only queued the pull request.
        match self.ports.forge.pull_request(&repo, number) {
            Ok(pr) if pr.state == PullRequestState::Open => self.queue_marked(removals),
            Ok(_) => self.merged(issue, number, head, auto),
            Err(e) => Ok(self.gate_failed(format!("cannot read #{number}: {e}"))),
        }
    }

    // Nobody is asked before a merge under `auto`, so a head the gates never
    // saw is adopted into the worktree and goes back through every gate.
    pub(super) fn regate(&mut self, from: &str, tip: &str) -> Result<(), String> {
        let item = self.current().expect("a gate is of a work item");
        let (repo, branch) = (&self.settings.git.checkout, &item.branch);
        worktree::adopt(repo, &item.worktree, branch, from, tip, &item.remote)
            .map_err(|e| format!("cannot bring the worktree to {}: {e}", short(tip)))?;
        let tip = Some(tip.to_owned());
        self.update(|item| {
            item.known.head = tip;
            item.phase = Phase::Review(Review::first());
        })
        .map_err(|e| e.to_string())
    }

    // The notice is saved with the merge, so it goes out exactly once.
    pub(super) fn merged(
        &mut self,
        issue: TicketId,
        number: u64,
        head: String,
        notice: bool,
    ) -> Result<Begin, StateError> {
        let mut next = self.state.clone();
        let Some(item) = self.current_in(&mut next) else {
            return Ok(Begin::Idle);
        };
        item.phase = Phase::Done {
            merged: true,
            closed: false,
        };
        let notice = notice.then_some(MergeNotice {
            issue,
            pull_request: number,
            head,
        });
        next.notices.extend(notice.clone().map(Notice::Merged));
        self.save(next)?;
        // A comment that fails is told once and not retried: the webhook's
        // retry is the notice's, and nothing here waits on the comment.
        if let Some(notice) = &notice {
            self.notice_on(number, &super::voice::merged_notice(notice));
        }
        self.finish(true)
    }

    fn withdraw(
        &mut self,
        issue: TicketId,
        number: u64,
        auto: bool,
        reason: String,
    ) -> Result<Begin, StateError> {
        let since = self.ports.clock.now();
        self.update(|item| item.phase = Phase::Ci { head: None, since })?;
        Ok(Begin::Report(if auto {
            StepReport::MergeWithdrawn {
                issue,
                pull_request: number,
                reason,
            }
        } else {
            StepReport::YesWithdrawn {
                issue,
                pull_request: number,
                reason,
            }
        }))
    }

    // The first refusal under `auto` goes back to CI, which catches the
    // branch up with `main`. A second one, on any head, asks.
    fn refused(
        &mut self,
        issue: TicketId,
        number: u64,
        head: String,
        reason: String,
    ) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let refused_before = item.merge_refused;
        let tried = Some(head.clone());
        self.update(|item| {
            item.merge_refused = true;
            item.merge_tried = tried;
        })?;
        if refused_before {
            return self.raise(number, Stuck::MergeRefused { head, why: reason }.into());
        }
        self.withdraw(issue, number, true, reason)
    }

    // Ends a work item that finished.
    pub(super) fn finish(&mut self, merged: bool) -> Result<Begin, StateError> {
        self.cleanup(merged)
    }

    // Removes the worktree, branch and build folder, then the work
    // item, and records its issue so the board does not take it again until the
    // forge closes it, or for one not merged, until it is ready again or edited. A pull
    // request left unmerged is handed back to the maintainer first.
    fn cleanup(&mut self, merged: bool) -> Result<Begin, StateError> {
        // First, since the findings sit in the build folder this removes.
        if merged && let Some(begin) = self.follow_ups()? {
            return Ok(begin);
        }
        // Before anything is removed, so a refused close runs this step again.
        if let (true, Some(item)) = (merged, self.current())
            && let Some(number) = item.pull_request
        {
            let issue = item.issue;
            if let Some(reason) = self.close_merged_issue(issue, number) {
                return Ok(self.gate_failed(reason));
            }
            self.close_done_parent_of(issue, number);
        }
        self.release_all()?;
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        if let (false, Some(number)) = (merged, item.pull_request)
            && let Err(reason) = self.hand_back(number)
        {
            return Ok(self.gate_failed(reason));
        }
        // Read before the build folder goes, which holds the file it is in.
        let friction = self.friction_of(item);
        let removed = worktree::remove(
            &self.settings.git.checkout,
            &item.worktree,
            &item.branch,
            &item.build,
            merged.then_some(&Remote::Origin),
        );
        let removed = removed.map_err(|e| e.to_string());
        if let Err(e) = removed.and_then(|()| self.remove_code_graph(item.issue)) {
            return Ok(self.gate_failed(format!("cannot clean up: {e}")));
        }
        // The item is gone, so a main it could not fetch warns about nothing.
        self.notes.ends(&main_fetch_key(item.issue));
        let now = self.ports.clock.now();
        let timings = item.split(now, self.timing_phase(item));
        let closed = matches!(item.phase, Phase::Done { closed: true, .. });
        let record = Finished {
            issue: item.issue,
            title: item.title.clone(),
            pull_request: item.pull_request,
            merged,
            closed,
            at: now,
            wall: timings.wall,
            seconds: timings.seconds.clone(),
            spend: Some(item.tally()),
        };
        let mut line = crate::usage::FinishedLine::of(&record, item, item.issue);
        if let Ok(agent) = self.worker_agent(item) {
            line.harness = Some(agent.harness.harness().as_str().to_owned());
            line.model = Some(agent.model);
        }
        let ended = record.clone();
        let report = StepReport::Finished {
            issue: item.issue,
            pull_request: item.pull_request,
            merged,
            closed,
            spend: Box::new(item.spend()),
            qwen: item.qwen,
            timings,
        };
        let issue = item.issue;
        let mut next = self.state.clone();
        next.work_items.retain(|open| open.issue != item.issue);
        // Rulings about this work item go with it, a question asked before
        // its pull request included.
        next.rulings.retain(|r| r.issue != Some(item.issue));
        // An issue whose item ended with no merged pull request, dropped or stopped
        // by a no, stays open, unlike a merged one the forge closes, so it goes on
        // its own list, which a label put back or an edit clears.
        let (listed, other) = match merged {
            false => (&mut next.dropped, &mut next.finished),
            true => (&mut next.finished, &mut next.dropped),
        };
        other.retain(|n| *n != item.issue);
        if !listed.contains(&item.issue) {
            listed.push(item.issue);
        }
        // Forgotten, so the board takes what the issue reads as then.
        next.dropped_text.remove(&item.issue);
        next.record_finished(record);
        self.save(next)?;
        self.ledger.append(&crate::usage::Line::Finished(line));
        if let Some(text) = friction {
            self.journal_at(now, Entry::Friction { issue, text });
        }
        self.tell_done(&ended);
        self.mark_held(issue, false);
        self.late_reads.remove(&issue);
        self.parked_reads.remove(&issue);
        Ok(Begin::Report(report))
    }
}

mod queue;

#[cfg(test)]
mod auto;
#[cfg(test)]
mod parent;

#[cfg(test)]
mod close;

#[cfg(test)]
mod tests;
