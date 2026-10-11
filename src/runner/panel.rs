//! The panel: four experts vote on each ruling before the maintainer is asked
//!
//! The work item's own worker, the project manager, the advisor and the
//! researcher each vote with `{"choice": ..., "why": ..., "design": ...}`
//! (shep-pm/shep-kelpie#461), blind to each other: no one's prompt carries
//! another's vote. The worker, the advisor and the researcher run on
//! threads of their own, together, in one flight; the project manager is
//! woken as it is for anything else, and answers when it does.
//!
//! A panel runs for a ruling once a project names an advisor or a
//! researcher, and the ruling lists two options or more. The ruling's comment
//! waits for its votes, for [`HOLD`] at most, and then lists them. A vote
//! kelpie cannot read, or an expert that could not be asked, is an
//! abstention, and is logged. An expert the project does not name is not on
//! the panel, and the worker abstains when its work item has no worker.
//!
//! The votes are kept on the ruling. Nothing decides by them yet (#466).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::Runner;
use super::pm::Wake;
use crate::panel::{Ballot, Vote, Voter};
use crate::ports::{AgentError, AgentReply, Timestamp};
use crate::state::Ruling;

mod ask;
mod calls;
mod folders;
mod history;
#[cfg(test)]
mod tests;

pub(super) use history::Record;

/// How long one expert's call may run before it is ended, in seconds
pub(super) const CEILING: u64 = 15 * 60;

/// How long a ruling's comment waits for its panel, in seconds: past an
/// expert's ceiling, so a call that was ended still gets its abstention heard
const HOLD: u64 = 20 * 60;

/// What the runner keeps about the panel, in memory only
///
/// A runner that restarts takes a ruling with no votes up again.
#[derive(Debug, Default)]
pub(super) struct Desk {
    // Each ruling the panel is voting on
    open: BTreeMap<u64, Open>,
    // The rulings whose experts have yet to be asked, oldest first
    queue: VecDeque<u64>,
    // Whether experts started since the loop last asked
    launched: bool,
}

// One ruling's vote so far.
#[derive(Debug)]
struct Open {
    since: Timestamp,
    // Who is still to vote
    waiting: BTreeSet<Voter>,
    ballots: BTreeMap<Voter, Option<Vote>>,
}

impl Desk {
    // The next ruling whose experts are to be asked. Its hold runs from now,
    // not from when it was taken up, since only one panel's experts run at a
    // time and a ruling queued behind another has not had its turn.
    pub(super) fn queue_pop(&mut self, now: Timestamp) -> Option<u64> {
        let id = self.queue.pop_front()?;
        if let Some(open) = self.open.get_mut(&id) {
            open.since = now;
        }
        Some(id)
    }

    // Who is still to vote on ruling `id`.
    pub(super) fn waiting(&self, id: u64) -> Vec<Voter> {
        (self.open.get(&id)).map_or_else(Vec::new, |o| o.waiting.iter().copied().collect())
    }
}

impl Runner {
    // Whether the project has a panel: it names an advisor or a researcher.
    pub(super) fn panel_enabled(&self) -> bool {
        self.agents.advisor.is_some() || self.agents.researcher.is_some()
    }

    /// Whether ruling `id`'s comment waits on its panel
    pub(super) fn panel_holds_ruling(&self, id: u64) -> bool {
        !self.draining && self.panel_enabled() && self.panel.open.contains_key(&id)
    }

    /// Takes up each ruling that needs its panel, settles each whose time ran
    /// out, and asks the experts of the oldest ruling waiting for them
    pub(super) fn panel_pass(&mut self) {
        self.panel_open_all();
        if self.panel_enabled() && self.panel_ask_next() {
            self.panel.launched = true;
        }
    }

    /// Takes up each ruling that needs its panel and settles each whose
    /// time ran out, so a ruling raised is held before its comment is posted
    pub(super) fn panel_open_all(&mut self) {
        if !self.panel_enabled() {
            // The settings dropped both experts since a ruling was taken up:
            // nothing waits on a panel that is no longer there.
            self.panel.open.clear();
            self.panel.queue.clear();
            self.panel_end_flight();
            return;
        }
        self.panel_forget_answered();
        self.panel_settle_overdue();
        if self.draining {
            return;
        }
        let wanting: Vec<u64> = (self.state.rulings.iter())
            .filter(|r| self.panel_wants(r))
            .map(|r| r.id)
            .collect();
        for id in wanting {
            self.panel_take_up(id);
        }
    }

    /// Whether the experts of a ruling started since this was last asked
    pub(super) fn panel_launched(&mut self) -> bool {
        std::mem::take(&mut self.panel.launched)
    }

    // Whether `ruling` needs a panel that has not been taken up: it has no
    // votes, lists two options or more, no hold of the project manager's is
    // still on it, and it is not posted already, since a comment posted
    // without the votes is not edited to hold them.
    fn panel_wants(&self, ruling: &Ruling) -> bool {
        let posted = ruling.alerted || self.state.heard.contains_key(&ruling.id);
        ruling.panel.is_none()
            && !posted
            && !self.panel.open.contains_key(&ruling.id)
            && !self.pm_holds_ruling(ruling.id)
            && super::voice::brief(ruling).1 >= 2
    }

    // Drops what the panel keeps of a ruling that is answered or gone.
    fn panel_forget_answered(&mut self) {
        let pending: BTreeSet<u64> = self.state.rulings.iter().map(|r| r.id).collect();
        self.panel.open.retain(|id, _| pending.contains(id));
        self.panel.queue.retain(|id| pending.contains(id));
    }

    // Closes each ruling's vote whose time ran out, its missing experts abstaining.
    fn panel_settle_overdue(&mut self) {
        let now = self.ports.clock.now();
        let overdue: Vec<u64> = (self.panel.open.iter())
            .filter(|(id, open)| {
                !self.panel.queue.contains(id) && now.0 >= open.since.0.saturating_add(HOLD)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in overdue {
            // Experts still in flight are ended, and abstain through their
            // calls' end, so no call runs on after its ruling is settled.
            let flying = self.panel_in_flight(id);
            if flying {
                self.panel_end_flight();
            }
            let waiting: Vec<Voter> = self.panel.open[&id].waiting.iter().copied().collect();
            for voter in waiting.into_iter().filter(|v| !flying || *v == Voter::Pm) {
                self.panel_abstains(id, voter, "it did not answer in time".into());
            }
        }
    }

    // Opens ruling `id`'s vote: who is on the panel, who abstains at once,
    // and who is asked.
    fn panel_take_up(&mut self, id: u64) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let issue = ruling.issue;
        let (worker, why) = self.panel_worker(issue);
        let now = self.ports.clock.now();
        let mut open = Open {
            since: now,
            waiting: BTreeSet::new(),
            ballots: BTreeMap::new(),
        };
        match worker {
            true => {
                open.waiting.insert(Voter::Worker);
            }
            false => {
                open.ballots.insert(Voter::Worker, None);
                self.notes
                    .push(format!("ruling {id}: the worker abstains, since {why}"));
            }
        }
        if self.agents.advisor.is_some() {
            open.waiting.insert(Voter::Advisor);
        }
        if self.agents.researcher.is_some() {
            open.waiting.insert(Voter::Researcher);
        }
        let asks = open.waiting.iter().any(|v| *v != Voter::Pm);
        if self.agents.pm.is_some() {
            open.waiting.insert(Voter::Pm);
            let asked = super::voice::brief(ruling).0;
            self.pm_due(Wake::Vote { id, asked });
        }
        self.panel.open.insert(id, open);
        match asks {
            true => self.panel.queue.push_back(id),
            false => self.panel_close_if_done(id),
        }
    }

    // Whether `issue`'s work item has a worker to ask, or why not.
    fn panel_worker(&self, issue: Option<u64>) -> (bool, String) {
        let Some(item) = issue.and_then(|n| self.state.item(n)) else {
            return (false, "the ruling has no work item open".into());
        };
        let ran = item
            .calls
            .iter()
            .any(|c| c.role == crate::ports::Role::Worker);
        match (ran, item.worktree.is_dir()) {
            (false, _) => (false, format!("#{} has had no worker turn", item.issue)),
            (_, false) => (false, format!("#{}'s worktree is gone", item.issue)),
            _ => (true, String::new()),
        }
    }

    /// Records `voter`'s `vote` on ruling `id`, or why it counts as none
    pub(super) fn panel_voted(&mut self, id: u64, voter: Voter, vote: Result<Vote, String>) {
        let Some(open) = self.panel.open.get_mut(&id) else {
            return;
        };
        if !open.waiting.remove(&voter) {
            return;
        }
        match vote {
            Ok(vote) => {
                open.ballots.insert(voter, Some(vote));
            }
            Err(why) => {
                open.ballots.insert(voter, None);
                self.notes.push(format!(
                    "ruling {id}: the {} abstains, since {why}",
                    voter.name().to_lowercase()
                ));
            }
        }
        self.panel_close_if_done(id);
    }

    /// Records that `voter` abstains on ruling `id`, since `why`
    pub(super) fn panel_abstains(&mut self, id: u64, voter: Voter, why: String) {
        self.panel_voted(id, voter, Err(why));
    }

    // Saves ruling `id`'s votes on it once none is still to come.
    fn panel_close_if_done(&mut self, id: u64) {
        if self
            .panel
            .open
            .get(&id)
            .is_none_or(|o| !o.waiting.is_empty())
        {
            return;
        }
        let Some(open) = self.panel.open.remove(&id) else {
            return;
        };
        let ballots: Vec<Ballot> = Voter::ALL
            .into_iter()
            .filter_map(|voter| {
                let vote = open.ballots.get(&voter)?.clone();
                Some(Ballot { voter, vote })
            })
            .collect();
        let told = ballots
            .iter()
            .map(|b| match &b.vote {
                Some(vote) => format!("{} {}", b.voter.name(), vote.choice),
                None => format!("{} abstained", b.voter.name()),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        ruling.panel = Some(ballots);
        match self.save(next) {
            Ok(()) => self
                .notes
                .push(format!("ruling {id}: the panel voted: {told}")),
            Err(e) => self
                .notes
                .push(format!("cannot save the panel's votes on ruling {id}: {e}")),
        }
    }

    /// Hears the project manager's reply to its vote wakes: each ruling it
    /// voted on, and each it was asked about and did not
    pub(super) fn panel_pm_answered(
        &mut self,
        asked: &[u64],
        votes: Vec<(u64, Result<Vote, String>)>,
    ) {
        let mut votes: BTreeMap<u64, Result<Vote, String>> = votes.into_iter().collect();
        for id in asked {
            let vote = votes
                .remove(id)
                .unwrap_or_else(|| Err("its reply held no vote on it".into()));
            self.panel_voted(*id, Voter::Pm, vote);
        }
    }

    /// Lets the panel go of a ruling the project manager can no longer be asked about
    pub(super) fn panel_pm_gone(&mut self, id: u64) {
        self.panel_abstains(id, Voter::Pm, "it could not be woken".into());
    }

    /// The option count of ruling `id`, for reading a vote on it
    pub(super) fn panel_options(&self, id: u64) -> usize {
        (self.state.rulings.iter().find(|r| r.id == id)).map_or(0, |r| super::voice::brief(r).1)
    }

    // Records how the experts' calls ended.
    pub(in crate::runner) fn panel_heard(
        &mut self,
        id: u64,
        options: usize,
        ends: Vec<(Voter, Result<AgentReply, AgentError>)>,
    ) {
        for (voter, result) in ends {
            let vote = match result {
                Ok(reply) => crate::panel::read(&reply.text, options),
                Err(e) => Err(format!("its call failed: {e}")),
            };
            self.panel_voted(id, voter, vote);
        }
        // Whoever did not answer at all abstains.
        let missing: Vec<Voter> = (self.panel.open.get(&id))
            .map(|o| {
                o.waiting
                    .iter()
                    .copied()
                    .filter(|v| *v != Voter::Pm)
                    .collect()
            })
            .unwrap_or_default();
        for voter in missing {
            self.panel_abstains(id, voter, "it was not asked".into());
        }
    }
}
