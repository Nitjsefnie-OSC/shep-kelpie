//! The panel: four experts vote on each ruling before the maintainer is asked
//!
//! The work item's own worker, the project manager, the advisor and the
//! researcher each vote blind: no one's prompt carries another's vote. The
//! worker, the advisor and the researcher run on threads of their own, in
//! one flight. The project manager is woken as for anything else. A panel
//! runs once a project names an advisor or a researcher, and the ruling
//! lists two options or more. A vote kelpie cannot read, or an expert it
//! could not ask, is a logged abstention. `rulings.decide` says whether the
//! verdict decides the ruling, informs the maintainer, or only measures.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::Runner;
use super::pm::Wake;
use crate::panel::{
    Advice, Ballot, Skip, Stage, Step, Tier, Verdict, Vote, Voter, Work, first_tier, settle,
    settle_always, simulate, work_of,
};
use crate::ports::{AgentError, AgentReply, Timestamp};
use crate::settings::{Decide, Merging, RulingName};
use crate::state::{Ruling, RulingKind};
use crate::ticket::TicketId;
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
    open: BTreeMap<TicketId, Open>,
    // The rulings whose experts have yet to be asked, oldest first
    queue: VecDeque<TicketId>,
    // Whether experts started since the loop last asked
    launched: bool,
    // How the last rulings were asked once they left `open`, for the votes
    // of a project manager wake that outlasts its ruling
    closed: BTreeMap<TicketId, Plan>,
}

// Why a voter ended by the maintainer's answer abstains.
const OVERTAKEN: &str = "the maintainer answered the ruling first";

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
    // The ruling as the maintainer answered it, while its panel votes on
    // under `shadow`: its votes go to its line in `rulings.jsonl`
    answered: Option<Ruling>,
    // Who was ended since the answer came before their vote
    overtaken: BTreeSet<Voter>,
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
    pub(super) fn mark(&self, ruling: TicketId, voter: Voter, choice: Option<char>) -> VoteMark {
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
    pub(super) fn queue_pop(&mut self, now: Timestamp) -> Option<TicketId> {
        let id = self.queue.pop_front()?;
        if let Some(open) = self.open.get_mut(&id) {
            open.since = now;
        }
        Some(id)
    }

    // How ruling `id`'s panel is being asked, or was when it closed.
    pub(super) fn plan(&self, id: TicketId) -> Option<Plan> {
        let open = self.open.get(&id).map(|o| &o.plan);
        open.or_else(|| self.closed.get(&id)).cloned()
    }

    // Keeps how ruling `id` was asked, now that its panel is done.
    fn retire(&mut self, id: TicketId, plan: Plan) {
        self.closed.insert(id, plan);
        while self.closed.len() > CLOSED_KEPT {
            self.closed.pop_first();
        }
    }

    // Who is still to vote on ruling `id`.
    pub(super) fn waiting(&self, id: TicketId) -> Vec<Voter> {
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
    pub(super) fn panel_holds_ruling(&self, id: TicketId) -> bool {
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
        let wanting: Vec<TicketId> = (self.state.rulings.iter())
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

    // Whether `ruling` needs a panel that has not been taken up. It is not a
    // contribution's, which never goes to a vote. It has no votes and lists
    // two options or more. A comment posted without the votes is never edited
    // to hold them, so a posted ruling is not voted on again.
    fn panel_wants(&self, ruling: &Ruling) -> bool {
        let posted = ruling.alerted || self.state.heard.contains_key(&ruling.id);
        !ruling.kind.is_contribution()
            && ruling.panel.is_none()
            && ruling.skipped.is_none()
            && ruling.unposted
            && !posted
            && !self.panel.open.contains_key(&ruling.id)
            && !self.pm_has_asked(ruling.id)
            && super::voice::brief(ruling).1 >= 2
            && !self.panel_decided_before(ruling)
    }

    // Whether the panel already decided a question like `ruling`'s.
    fn panel_decided_before(&self, ruling: &Ruling) -> bool {
        matches!(ruling.kind, RulingKind::Question { .. })
            && (history::records(&self.paths.rulings).iter()).any(|r| r.decided_by_panel(ruling))
    }

    // Drops what the panel keeps of a ruling that is answered or gone.
    fn panel_forget_answered(&mut self) {
        let mut pending: BTreeSet<TicketId> = self.state.rulings.iter().map(|r| r.id).collect();
        // A ruling answered under `shadow` keeps its panel to the end of its hold.
        pending.extend(
            (self.panel.open.iter())
                .filter(|(_, open)| open.answered.is_some())
                .map(|(id, _)| *id),
        );
        // Experts in flight for a ruling that is gone are ended, so a call
        // does not run on in a session the answer moves on.
        if (self.flights.panel_ruling()).is_some_and(|id| !pending.contains(&id)) {
            self.panel_end_flight();
        }
        let gone: Vec<TicketId> = (self.panel.open.keys())
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
        let draining = self.draining();
        let overdue: Vec<TicketId> = (self.panel.open.iter())
            .filter(|(id, open)| {
                let waits_its_turn = self.panel.queue.contains(id) && !draining;
                !waits_its_turn && now.0 >= open.since.0.saturating_add(HOLD)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in overdue {
            if self.panel.queue.contains(&id) {
                let voted = self.panel.open[&id].ballots.values().any(Option::is_some);
                if !voted {
                    self.panel_skip(id, Skip::Draining);
                    continue;
                }
                self.panel.queue.retain(|queued| *queued != id);
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

    // Lets ruling `id` go to the maintainer with no vote, and keeps why. A reason
    // that cannot be saved leaves the ruling with the panel, to be skipped again
    // next step, so none goes unvoted with no reason on record.
    fn panel_skip(&mut self, id: TicketId, skip: Skip) {
        let answered = self
            .panel
            .open
            .get(&id)
            .is_some_and(|o| o.answered.is_some());
        let kept = match answered {
            true => self.panel_skip_answered(id, skip),
            false => self.panel_skipped(id, skip),
        };
        if !kept {
            return;
        }
        if let Some(open) = self.panel.open.remove(&id) {
            self.panel.retire(id, open.plan);
        }
        self.panel.queue.retain(|queued| *queued != id);
    }

    // Keeps on answered ruling `id`'s line that the panel cast no vote on it,
    // since `skip`, and returns whether it is saved.
    fn panel_skip_answered(&mut self, id: TicketId, skip: Skip) -> bool {
        let field = serde_json::to_value(skip).unwrap_or_default();
        match history::amend(&self.paths.rulings, id, vec![("skipped", field)]) {
            Ok(()) => true,
            Err(e) => {
                self.notes.push(format!(
                    "cannot save why the panel skipped ruling {id}: {e}"
                ));
                false
            }
        }
    }

    /// Keeps on ruling `id` that the panel cast no vote on it, since `skip`,
    /// and returns whether it is saved
    pub(super) fn panel_skipped(&mut self, id: TicketId, skip: Skip) -> bool {
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return false;
        };
        ruling.skipped = Some(skip);
        match self.save(next) {
            Ok(()) => {
                self.notes.push(format!(
                    "ruling {id}: the panel skips it, since {}",
                    skip.why()
                ));
                true
            }
            Err(e) => {
                self.notes.push(format!(
                    "cannot save why the panel skipped ruling {id}: {e}"
                ));
                false
            }
        }
    }

    /// Keeps why the panel cast no vote on ruling `id`, whose comment is posted
    /// now, when the comment had gone out before the panel took it up
    pub(super) fn panel_unvoted(&mut self, id: TicketId) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let voted = ruling.panel.is_some() || ruling.skipped.is_some();
        let posted = ruling.alerted || self.state.heard.contains_key(&id);
        if self.panel_enabled() && !voted && super::voice::brief(ruling).1 >= 2 {
            let skip = match self.panel_decided_before(ruling) {
                true => Skip::Repeated,
                false => Skip::Posted,
            };
            if posted || skip == Skip::Repeated {
                self.panel_skipped(id, skip);
            }
        }
    }

    // Opens ruling `id`'s vote: its work item's type, who is on the panel,
    // who abstains at once, and who is asked.
    fn panel_take_up(&mut self, id: TicketId) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let (kind, ticket, number) = (
            RulingName::of(&ruling.kind),
            ruling.issue,
            ruling.pull_request,
        );
        let asked = super::voice::brief(ruling).0;
        let work = self.panel_work(id, number);
        let always = self.settings.rulings.decide == Decide::Always;
        // `full_panel_paths` has no effect under `always`: it asks all four anyway.
        let risky = !always && self.panel_risky(id, ticket);
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
            answered: None,
            overtaken: BTreeSet::new(),
        };
        let open = self.panel_enlist(id, ticket, &asked, open);
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
    fn panel_enlist(
        &mut self,
        id: TicketId,
        ticket: Option<TicketId>,
        asked: &str,
        mut open: Open,
    ) -> Open {
        for &voter in open.plan.stage.voters() {
            if open.ballots.contains_key(&voter) || open.waiting.contains(&voter) {
                continue;
            }
            match voter {
                Voter::Worker => match self.panel_worker(ticket) {
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
    fn panel_work(&mut self, id: TicketId, number: Option<u64>) -> Work {
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

    // Whether ruling `id`'s diff from `main`'s merge base touches
    // `rulings.full_panel_paths`. It is read from `main`, not the review
    // base: an adopted pull request's review leaves out the commits it
    // arrived with. A diff that cannot be read counts as touching one.
    fn panel_risky(&mut self, id: TicketId, ticket: Option<TicketId>) -> bool {
        let globs = self.settings.rulings.full_panel_paths.clone();
        let Some(item) = ticket
            .and_then(|t| self.state.item(t))
            .filter(|_| !globs.is_empty())
        else {
            return false;
        };
        let (repo, worktree) = (&self.settings.git.checkout, &item.worktree);
        let base = format!("origin/{}", self.item_base(item));
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

    // Whether `ticket`'s work item has a worker to ask, or why not.
    fn panel_worker(&self, ticket: Option<TicketId>) -> (bool, String) {
        let no_item = || (false, "the ruling has no work item open".to_owned());
        let Some(ticket) = ticket else {
            return no_item();
        };
        let Some(item) = self.state.item(ticket) else {
            return no_item();
        };
        let ran = item
            .calls
            .iter()
            .any(|c| c.role == crate::ports::Role::Worker);
        match (ran, item.worktree.is_dir()) {
            (false, _) => (false, format!("#{ticket} has had no worker turn")),
            (_, false) => (false, format!("#{ticket}'s worktree is gone")),
            _ => (true, String::new()),
        }
    }

    /// Records `voter`'s `vote` on ruling `id`, or why it counts as none.
    /// Whether the ballot landed: a ruling that is closed, or one that has
    /// `voter`'s ballot already, takes none.
    pub(super) fn panel_voted(
        &mut self,
        id: TicketId,
        voter: Voter,
        vote: Result<Vote, String>,
    ) -> bool {
        let Some(open) = self.panel.open.get_mut(&id) else {
            return false;
        };
        if !open.waiting.remove(&voter) {
            return false;
        }
        match vote {
            Ok(vote) => {
                open.ballots.insert(voter, Some(vote));
            }
            Err(why) => {
                let why = match open.overtaken.contains(&voter) {
                    true => OVERTAKEN.to_owned(),
                    false => why,
                };
                open.ballots.insert(voter, None);
                self.notes.push(format!(
                    "ruling {id}: the {} abstains, since {why}",
                    voter.name().to_lowercase()
                ));
            }
        }
        self.panel_close_if_done(id);
        true
    }

    /// Records that `voter` abstains on ruling `id`, since `why`
    pub(super) fn panel_abstains(&mut self, id: TicketId, voter: Voter, why: String) {
        self.panel_voted(id, voter, Err(why));
    }

    // Settles ruling `id` once none of the experts its stage asks is still to
    // vote: it goes up a stage when the panel does not decide, else its votes
    // are saved and it is decided or left to the maintainer.
    fn panel_close_if_done(&mut self, id: TicketId) {
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
                match &open.answered {
                    Some(_) => self.panel_record_late(id, &open, &ballots),
                    None => self.panel_conclude(id, &open.plan, ballots),
                }
                self.panel.retire(id, open.plan);
                return;
            };
            // Asked again, a stage up: its hold runs from when its turn comes.
            let Some(mut open) = self.panel.open.remove(&id) else {
                return;
            };
            open.plan.stage = up;
            let ruling = self.state.rulings.iter().find(|r| r.id == id);
            let (ticket, asked) = ruling.map_or((None, String::new()), |r| {
                (r.issue, super::voice::brief(r).0)
            });
            let open = self.panel_enlist(id, ticket, &asked, open);
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
    fn panel_conclude(&mut self, id: TicketId, plan: &Plan, ballots: Vec<Ballot>) {
        let told = told(&ballots);
        let merge = (self.state.rulings.iter())
            .any(|r| r.id == id && matches!(r.kind, RulingKind::Merge { .. }));
        let step = match (plan.shadow, plan.always) {
            (true, _) => simulate(plan.start, plan.tier, &ballots).1,
            (_, true) => settle_always(&ballots, merge),
            _ => settle(plan.stage, plan.tier, &ballots),
        };
        let advice = advice_of(step);
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        ruling.panel = Some(ballots);
        ruling.verdict = Some(verdict_of(plan, advice));
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

    // Saves the votes of a ruling the maintainer answered while they were
    // being cast, on its line in `rulings.jsonl`, where `status` counts them
    // against the answer. Under `shadow` they decide nothing.
    fn panel_record_late(&mut self, id: TicketId, open: &Open, ballots: &[Ballot]) {
        let plan = &open.plan;
        let advice = advice_of(simulate(plan.start, plan.tier, ballots).1);
        let overtaken: Vec<Voter> = (ballots.iter())
            .filter(|b| b.vote.is_none() && open.overtaken.contains(&b.voter))
            .map(|b| b.voter)
            .collect();
        let mut fields = vec![
            ("ballots", serde_json::to_value(ballots).unwrap_or_default()),
            (
                "verdict",
                serde_json::to_value(verdict_of(plan, advice)).unwrap_or_default(),
            ),
        ];
        if !overtaken.is_empty() {
            fields.push((
                "overtaken",
                serde_json::to_value(overtaken).unwrap_or_default(),
            ));
        }
        match history::amend(&self.paths.rulings, id, fields) {
            Ok(()) => self.notes.push(format!(
                "ruling {id}: the panel voted after the answer: {}",
                told(ballots)
            )),
            Err(e) => self.notes.push(format!(
                "cannot save the panel's votes on answered ruling {id}: {e}"
            )),
        }
    }

    /// Keeps the panel of ruling `id` voting after the maintainer's answer to
    /// `ruling`, now saved, when the project decides by `shadow`
    pub(super) fn panel_outlives(&mut self, ruling: Ruling) {
        let id = ruling.id;
        // A worker whose call already failed has its own reason to abstain.
        let worker_open = self.panel_worker_open(id);
        let Some(open) = self.panel.open.get_mut(&id).filter(|o| o.plan.shadow) else {
            return;
        };
        open.answered = Some(ruling);
        // The answer moves its work item on at once, and the worker's vote
        // shares that item's session, so the worker does not wait it out.
        let overtaken = open.waiting.contains(&Voter::Worker) && worker_open;
        if overtaken {
            open.overtaken.insert(Voter::Worker);
        }
        if !self.panel_release_worker(id) && overtaken {
            self.panel_abstains(id, Voter::Worker, OVERTAKEN.into());
        }
    }

    // Ruling `id` as the panel votes on it: pending, or answered under `shadow`.
    pub(super) fn panel_ruling(&self, id: TicketId) -> Option<Ruling> {
        let pending = self.state.rulings.iter().find(|r| r.id == id);
        let answered = self.panel.open.get(&id).and_then(|o| o.answered.as_ref());
        pending.or(answered).cloned()
    }

    /// Hears the project manager's reply to its vote wakes: each ruling it
    /// voted on, and each it was asked about and did not
    /// Records the project manager's votes on the rulings it was asked about,
    /// an abstention for each it gave none on. What each ballot did, in words,
    /// for the log: a ruling that closed before its vote landed took none.
    pub(super) fn panel_pm_answered(
        &mut self,
        asked: &[TicketId],
        votes: Vec<(TicketId, Result<Vote, String>)>,
    ) -> Vec<String> {
        let mut votes: BTreeMap<TicketId, Result<Vote, String>> = votes.into_iter().collect();
        let mut done = Vec::new();
        for id in asked {
            let vote = votes
                .remove(id)
                .unwrap_or_else(|| Err("its reply held no vote on it".into()));
            let said = match &vote {
                Ok(vote) => format!("voted ruling {id} for option {}", vote.choice),
                Err(why) => format!("abstained on ruling {id}: {why}"),
            };
            if self.panel_voted(*id, Voter::Pm, vote) {
                done.push(said);
            }
        }
        done
    }

    /// Lets the panel go of a ruling the project manager can no longer be asked about
    pub(super) fn panel_pm_gone(&mut self, id: TicketId) {
        self.panel_abstains(id, Voter::Pm, "it could not be woken".into());
    }

    /// The option count of ruling `id`, for reading a vote on it
    pub(super) fn panel_options(&self, id: TicketId) -> usize {
        self.panel_ruling(id)
            .map_or(0, |r| super::voice::brief(&r).1)
    }

    // Records how the experts' calls ended.
    pub(in crate::runner) fn panel_heard(
        &mut self,
        id: TicketId,
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

// What the ballots say, for the log.
fn told(ballots: &[Ballot]) -> String {
    ballots
        .iter()
        .map(|b| match &b.vote {
            Some(vote) => format!("{} {}", b.voter.name(), vote.choice),
            None => format!("{} abstained", b.voter.name()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// The advice a step stands behind, if any.
fn advice_of(step: Step) -> Option<Advice> {
    match step {
        Step::Decided(advice) => Some(advice),
        Step::Maintainer { advice, .. } => advice,
        Step::Up(_) => None,
    }
}

// What a panel of `plan` concluded, with `advice`.
fn verdict_of(plan: &Plan, advice: Option<Advice>) -> Verdict {
    Verdict {
        work: plan.work.kind.clone(),
        tier: plan.work.tier,
        advice,
        risky: plan.risky,
        shadow: plan.shadow,
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
