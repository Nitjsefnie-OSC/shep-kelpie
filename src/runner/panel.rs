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
//! A runner that drains asks no expert, so a ruling raised in a drain waits
//! for the runner that restarts or undrains, and takes its panel then. One
//! still waiting after [`HOLD`] goes to the maintainer with no vote, and the
//! reason is kept on it ([`Skip`]) for `rulings.jsonl` and `status`.
//!
//! The votes and the verdict they reach are kept on the ruling. Under
//! `rulings.decide = "panel"` the verdict decides it; under `"always"` the
//! most votes of all four decide it unless no expert voted; under `"shadow"`
//! it only informs the maintainer's comment and the measurements (#466).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::Runner;
use super::pm::Wake;
use crate::panel::{
    Ballot, Skip, Stage, Step, Tier, Verdict, Vote, Voter, Work, first_tier, settle, settle_always,
    simulate, work_of,
};
use crate::ports::{AgentError, AgentReply, Timestamp};
use crate::settings::{Decide, Merging, RulingName};
use crate::state::{Ruling, RulingKind};
use crate::usage::VoteMark;

mod ask;
mod calls;
mod decided;
mod folders;
mod history;
pub use measure::Standing;
mod measure;
#[cfg(test)]
mod tests;

pub(super) use history::{Answerer, Record, records};

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
    // How the last rulings were asked once they left `open`, for the votes
    // of a project manager wake that outlasts its ruling
    closed: BTreeMap<u64, Plan>,
}

// How many closed rulings' plans are kept.
const CLOSED_KEPT: usize = 32;

// One ruling's vote so far.
#[derive(Debug)]
struct Open {
    since: Timestamp,
    // Who is still to vote
    waiting: BTreeSet<Voter>,
    ballots: BTreeMap<Voter, Option<Vote>>,
    plan: Plan,
}

// How a ruling's panel is asked.
#[derive(Debug, Clone)]
pub(super) struct Plan {
    // The kind of ruling it is
    kind: RulingName,
    // The work item's type, and the panel it sets
    work: Work,
    // Whether its diff touches a path on `rulings.full_panel_paths`
    risky: bool,
    // The panel it starts at: all four for a question or a risky diff, else
    // the one the type sets
    tier: Tier,
    // Under `shadow` every expert is asked at once and nothing is decided
    shadow: bool,
    // Under `always` every expert is asked at once and the most votes decide
    always: bool,
    // Where the panel starts, and where it is now
    start: Stage,
    stage: Stage,
}

impl Plan {
    /// A plan that asks all four at once for a ruling of `kind` on work of
    /// type `work`, as `shadow` does
    #[cfg(test)]
    pub(super) fn shadow(kind: RulingName, work: Work) -> Self {
        let stage = Stage::Tier(Tier::Full);
        Self {
            kind,
            work,
            risky: false,
            tier: Tier::Full,
            shadow: true,
            always: false,
            start: stage,
            stage,
        }
    }

    /// What the usage ledger says of a vote on this ruling
    pub(super) fn mark(&self, ruling: u64, voter: Voter, choice: Option<char>) -> VoteMark {
        VoteMark {
            ruling,
            ruling_kind: self.kind,
            voter,
            panel: self.work.tier,
            work: self.work.kind.clone(),
            choice,
        }
    }
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

    // How ruling `id`'s panel is being asked, or was when it closed.
    pub(super) fn plan(&self, id: u64) -> Option<Plan> {
        let open = self.open.get(&id).map(|o| &o.plan);
        open.or_else(|| self.closed.get(&id)).cloned()
    }

    // Keeps how ruling `id` was asked, now that its panel is done.
    fn retire(&mut self, id: u64, plan: Plan) {
        self.closed.insert(id, plan);
        while self.closed.len() > CLOSED_KEPT {
            self.closed.pop_first();
        }
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

    /// How the panel stands against the maintainer's answers, for `status`;
    /// none for a project with no panel
    pub(super) fn panel_status(&self) -> Option<Standing> {
        self.panel_enabled().then(|| {
            let records = history::records(&self.paths.rulings);
            // The ledger is read only when some ruling can be scored with it.
            let scorable = (records.iter()).any(|r| r.verdict.as_ref().is_some_and(|v| v.shadow));
            // A ledger that cannot be read leaves the costs unknown, not zero.
            let costs = match scorable {
                true => (crate::usage::read(self.ledger.path()))
                    .ok()
                    .map(|lines| crate::usage::vote_costs(&lines)),
                false => Some(BTreeMap::new()),
            };
            measure::standing(self.settings.rulings.decide, &records, costs.as_ref())
        })
    }

    /// Whether ruling `id`'s comment waits on its panel
    pub(super) fn panel_holds_ruling(&self, id: u64) -> bool {
        self.panel_enabled() && self.panel.open.contains_key(&id)
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
    // still on it, and its comment is still to be posted, since one posted
    // without the votes is not edited to hold them and a restart must not
    // ask the experts again for a vote nobody would see.
    fn panel_wants(&self, ruling: &Ruling) -> bool {
        let posted = ruling.alerted || self.state.heard.contains_key(&ruling.id);
        ruling.panel.is_none()
            && ruling.skipped.is_none()
            && ruling.unposted
            && !posted
            && !self.panel.open.contains_key(&ruling.id)
            && !self.pm_holds_ruling(ruling.id)
            && super::voice::brief(ruling).1 >= 2
    }

    // Drops what the panel keeps of a ruling that is answered or gone.
    fn panel_forget_answered(&mut self) {
        let pending: BTreeSet<u64> = self.state.rulings.iter().map(|r| r.id).collect();
        // Experts in flight for a ruling that is gone are ended, so a call
        // does not run on in a session the answer moves on.
        if (self.flights.panel_ruling()).is_some_and(|id| !pending.contains(&id)) {
            self.panel_end_flight();
        }
        let gone: Vec<u64> = (self.panel.open.keys())
            .copied()
            .filter(|id| !pending.contains(id))
            .collect();
        for id in gone {
            if let Some(open) = self.panel.open.remove(&id) {
                self.panel.retire(id, open.plan);
            }
        }
        self.panel.queue.retain(|id| pending.contains(id));
    }

    // Closes each ruling's vote whose time ran out, its missing experts abstaining.
    // A ruling still queued has had no turn, and in a drain it never will: one
    // with no vote goes to the maintainer without the panel.
    fn panel_settle_overdue(&mut self) {
        let now = self.ports.clock.now();
        let draining = self.draining;
        let overdue: Vec<u64> = (self.panel.open.iter())
            .filter(|(id, open)| {
                let waits_its_turn = self.panel.queue.contains(id) && !draining;
                !waits_its_turn && now.0 >= open.since.0.saturating_add(HOLD)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in overdue {
            if self.panel.queue.contains(&id) {
                self.panel.queue.retain(|queued| *queued != id);
                let voted = self.panel.open[&id].ballots.values().any(Option::is_some);
                if !voted {
                    self.panel_skip(id, Skip::Draining);
                    continue;
                }
            }
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

    // Lets ruling `id` go to the maintainer with no vote, and keeps why.
    fn panel_skip(&mut self, id: u64, skip: Skip) {
        if let Some(open) = self.panel.open.remove(&id) {
            self.panel.retire(id, open.plan);
        }
        self.panel.queue.retain(|queued| *queued != id);
        self.panel_skipped(id, skip);
    }

    /// Keeps on ruling `id` that the panel cast no vote on it, since `skip`
    pub(super) fn panel_skipped(&mut self, id: u64, skip: Skip) {
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        ruling.skipped = Some(skip);
        match self.save(next) {
            Ok(()) => self.notes.push(format!(
                "ruling {id}: the panel skips it, since {}",
                skip.why()
            )),
            Err(e) => self.notes.push(format!(
                "cannot save why the panel skipped ruling {id}: {e}"
            )),
        }
    }

    /// Keeps why the panel cast no vote on ruling `id`, whose comment is posted
    /// now, when the comment had gone out before the panel took it up
    pub(super) fn panel_unvoted(&mut self, id: u64) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let voted = ruling.panel.is_some() || ruling.skipped.is_some();
        let posted = ruling.alerted || self.state.heard.contains_key(&id);
        if self.panel_enabled() && !voted && posted && super::voice::brief(ruling).1 >= 2 {
            self.panel_skipped(id, Skip::Posted);
        }
    }

    // Opens ruling `id`'s vote: its work item's type, who is on the panel,
    // who abstains at once, and who is asked.
    fn panel_take_up(&mut self, id: u64) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let (kind, issue, number) = (
            RulingName::of(&ruling.kind),
            ruling.issue,
            ruling.pull_request,
        );
        let asked = super::voice::brief(ruling).0;
        let work = self.panel_work(id, number);
        let always = self.settings.rulings.decide == Decide::Always;
        // `full_panel_paths` has no effect under `always`: it asks all four anyway.
        let risky = !always && self.panel_risky(id, issue);
        let tier = first_tier(work.tier, kind == RulingName::Question, risky);
        let shadow = self.settings.rulings.decide == Decide::Shadow;
        // A kind the project manager decides alone starts with it alone, if it is
        // there, unless the ruling is raised to the full panel: a question or a
        // risky path is never one voice's, whatever `pm_alone` lists.
        let raised = kind == RulingName::Question || risky;
        let alone = self.settings.rulings.pm_decides(kind) && self.agents.pm.is_some() && !raised;
        let alone = alone && !always;
        let first = match alone {
            true => Stage::Alone,
            false => Stage::Tier(tier),
        };
        // Under shadow and always all four are asked at once.
        let stage = match shadow || always {
            true => Stage::Tier(Tier::Full),
            false => first,
        };
        let plan = Plan {
            kind,
            work,
            risky,
            tier,
            shadow,
            always,
            start: first,
            stage,
        };
        let open = Open {
            since: self.ports.clock.now(),
            waiting: BTreeSet::new(),
            ballots: BTreeMap::new(),
            plan,
        };
        let open = self.panel_enlist(id, issue, &asked, open);
        let asks = open.waiting.iter().any(|v| *v != Voter::Pm);
        self.panel.open.insert(id, open);
        match asks {
            true => self.panel.queue.push_back(id),
            false => self.panel_close_if_done(id),
        }
    }

    // Adds who `open`'s stage asks and has not been asked: an expert that is
    // there is waited for (and the project manager is woken), one that is not
    // abstains, or, when the project does not name it, is left off the panel.
    fn panel_enlist(&mut self, id: u64, issue: Option<u64>, asked: &str, mut open: Open) -> Open {
        for &voter in open.plan.stage.voters() {
            if open.ballots.contains_key(&voter) || open.waiting.contains(&voter) {
                continue;
            }
            match voter {
                Voter::Worker => match self.panel_worker(issue) {
                    (true, _) => {
                        open.waiting.insert(voter);
                    }
                    (false, why) => {
                        open.ballots.insert(voter, None);
                        self.notes
                            .push(format!("ruling {id}: the worker abstains, since {why}"));
                    }
                },
                Voter::Advisor if self.agents.advisor.is_some() => {
                    open.waiting.insert(voter);
                }
                Voter::Researcher if self.agents.researcher.is_some() => {
                    open.waiting.insert(voter);
                }
                Voter::Pm if self.agents.pm.is_some() => {
                    open.waiting.insert(voter);
                    let asked = asked.to_owned();
                    self.pm_due(Wake::Vote { id, asked });
                }
                _ => {}
            }
        }
        open
    }

    // What the pull request's title says of ruling `id`'s work item, which
    // sets the panel asked; any work with no title read is all four's.
    fn panel_work(&mut self, id: u64, number: Option<u64>) -> Work {
        let Some(number) = number else {
            return work_of(None);
        };
        match self.ports.forge.reviewed(&self.remote, number) {
            Ok(pr) => work_of(Some(&pr.title)),
            Err(e) => {
                self.notes.push(format!(
                    "ruling {id}: cannot read the title of #{number}, so its panel is all four: {e}"
                ));
                work_of(None)
            }
        }
    }

    // Whether the diff of ruling `id`'s work item from `main`'s merge base touches
    // a path on `rulings.full_panel_paths`, which calls the full panel. It is
    // read from `main`, not the review base: an adopted pull request's review
    // leaves out the commits it arrived with, and those are as risky. With
    // no list, or no work item, there is nothing to read. A diff that cannot
    // be read counts as touching one, since this only ever raises scrutiny.
    fn panel_risky(&mut self, id: u64, issue: Option<u64>) -> bool {
        let globs = self.settings.rulings.full_panel_paths.clone();
        let Some(item) = issue
            .and_then(|n| self.state.item(n))
            .filter(|_| !globs.is_empty())
        else {
            return false;
        };
        let (repo, worktree) = (&self.settings.git.checkout, &item.worktree);
        let base = format!("origin/{}", crate::worktree::BASE);
        let changed = super::review::changed_files(repo, worktree, &base);
        match changed {
            Ok(files) => files
                .iter()
                .any(|file| globs.iter().any(|glob| super::review::matches(glob, file))),
            Err(e) => {
                self.notes.push(format!(
                    "ruling {id}: cannot read the diff of its work item, so its panel is all \
                     four: {e}"
                ));
                true
            }
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

    // Settles ruling `id` once none of the experts its stage asks is still to
    // vote: it goes up a stage when the panel does not decide, else its votes
    // are saved and it is decided or left to the maintainer.
    fn panel_close_if_done(&mut self, id: u64) {
        loop {
            let Some(open) = self.panel.open.get(&id).filter(|o| o.waiting.is_empty()) else {
                return;
            };
            let ballots = ballots_of(open);
            let up = match open.plan.shadow || open.plan.always {
                true => None,
                false => match settle(open.plan.stage, open.plan.tier, &ballots) {
                    Step::Up(up) => Some(up),
                    _ => None,
                },
            };
            let Some(up) = up else {
                let Some(open) = self.panel.open.remove(&id) else {
                    return;
                };
                self.panel_conclude(id, &open.plan, ballots);
                self.panel.retire(id, open.plan);
                return;
            };
            // Asked again, a stage up: its hold runs from when its turn comes.
            let Some(mut open) = self.panel.open.remove(&id) else {
                return;
            };
            open.plan.stage = up;
            let ruling = self.state.rulings.iter().find(|r| r.id == id);
            let (issue, asked) = ruling.map_or((None, String::new()), |r| {
                (r.issue, super::voice::brief(r).0)
            });
            let open = self.panel_enlist(id, issue, &asked, open);
            let asks = open.waiting.iter().any(|v| *v != Voter::Pm);
            self.panel.open.insert(id, open);
            if asks {
                self.panel.queue.push_back(id);
                return;
            }
        }
    }

    // Saves ruling `id`'s votes and what they come to, and, under `panel`,
    // applies the option they decide on, unless the maintainer is to be asked.
    fn panel_conclude(&mut self, id: u64, plan: &Plan, ballots: Vec<Ballot>) {
        let told = ballots
            .iter()
            .map(|b| match &b.vote {
                Some(vote) => format!("{} {}", b.voter.name(), vote.choice),
                None => format!("{} abstained", b.voter.name()),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let merge = (self.state.rulings.iter())
            .any(|r| r.id == id && matches!(r.kind, RulingKind::Merge { .. }));
        let step = match (plan.shadow, plan.always) {
            (true, _) => simulate(plan.start, plan.tier, &ballots).1,
            (_, true) => settle_always(&ballots, merge),
            _ => settle(plan.stage, plan.tier, &ballots),
        };
        let advice = match step {
            Step::Decided(advice) => Some(advice),
            Step::Maintainer { advice, .. } => advice,
            Step::Up(_) => None,
        };
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        ruling.panel = Some(ballots);
        ruling.verdict = Some(Verdict {
            work: plan.work.kind.clone(),
            tier: plan.work.tier,
            advice,
            risky: plan.risky,
            shadow: plan.shadow,
        });
        // A merge under `ask` is the maintainer's whatever the panel says.
        let asks_merge = matches!(ruling.kind, RulingKind::Merge { .. })
            && self.settings.git.merging == Merging::Ask;
        match self.save(next) {
            Ok(()) => self
                .notes
                .push(format!("ruling {id}: the panel voted: {told}")),
            Err(e) => {
                self.notes
                    .push(format!("cannot save the panel's votes on ruling {id}: {e}"));
                return;
            }
        }
        match (step, plan.shadow, asks_merge) {
            (Step::Decided(advice), false, false) => self.panel_decides(id, advice),
            (Step::Maintainer { design: true, .. }, false, _) => self.notes.push(format!(
                "ruling {id}: an expert flagged design, so the maintainer decides"
            )),
            _ => {}
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
        // Whoever this flight was to hear from and gave no end abstains, once
        // the rest are heard. Found first: a stage up asks others in the meantime.
        let heard: Vec<Voter> = ends.iter().map(|(voter, _)| *voter).collect();
        let missing: Vec<Voter> = (self.panel.open.get(&id))
            .map(|o| {
                (o.waiting.iter().copied())
                    .filter(|v| *v != Voter::Pm && !heard.contains(v))
                    .collect()
            })
            .unwrap_or_default();
        for (voter, result) in ends {
            let vote = vote_of(&result, options);
            self.panel_voted(id, voter, vote);
        }
        for voter in missing {
            self.panel_abstains(id, voter, "it was not asked".into());
        }
    }
}

// The ballots of `open` in the order the comment lists them.
fn ballots_of(open: &Open) -> Vec<Ballot> {
    Voter::ALL
        .into_iter()
        .filter_map(|voter| {
            let vote = open.ballots.get(&voter)?.clone();
            Some(Ballot { voter, vote })
        })
        .collect()
}

/// The vote an expert's call came back with, or why it gave none
pub(super) fn vote_of(
    result: &Result<AgentReply, AgentError>,
    options: usize,
) -> Result<Vote, String> {
    match result {
        Ok(reply) => crate::panel::read(&reply.text, options),
        Err(e) => Err(format!("its call failed: {e}")),
    }
}
