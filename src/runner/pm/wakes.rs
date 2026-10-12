//! What wakes the project manager besides a pick and a `tell`: a work item
//! parked on a stuck item's ruling, a pair of open branches newly in
//! conflict, and, once it is back, whatever it missed while it was not

use std::collections::BTreeMap;

use super::{Pick, Wake, answer};
use crate::panel::{Tier, Voter};
use crate::ports::{AgentError, AgentReply};
use crate::runner::Runner;
use crate::settings::RulingName;
use crate::state::{ProjectState, RulingKind, Stuck};
use crate::usage::VoteMark;
use crate::work_item::Phase;

impl Runner {
    // Queues a wake for each item still stuck and each conflict still open,
    // once it is back after giving some up.
    pub(super) fn still_due(&mut self) {
        let mut wakes = Vec::new();
        for item in &self.state.work_items {
            let issue = item.issue;
            let parked = match item.phase {
                Phase::Ruling { id } => self.state.rulings.iter().find(|r| r.id == id),
                _ => None,
            };
            if let Some(wake) = parked.and_then(|r| stuck_wake(issue, r.id, &r.kind)) {
                wakes.push(wake);
            } else if self.flights.watched(issue).is_some_and(|w| w.idle) {
                let what = "its worker has shown no tool call or output for 10 minutes or more";
                wakes.push(Wake::Stuck(issue, what.to_owned()));
            }
        }
        for (&(a, b), files) in &self.pm.conflicts {
            wakes.push(Wake::Conflict(a, b, files.clone()));
        }
        for wake in wakes {
            self.pm_due(wake);
        }
    }

    /// The votes the wake in flight casts, for its line in the usage ledger:
    /// one for each ruling it was asked to vote on, with its choice where
    /// `result` holds one and none where the project manager abstained. A
    /// wake that failed, or whose reply is unread, casts none: it is asked
    /// again, and its retry writes them.
    pub(in crate::runner) fn pm_vote_marks(
        &self,
        result: &Result<AgentReply, AgentError>,
    ) -> Vec<VoteMark> {
        let Some(flying) = &self.pm.flying else {
            return Vec::new();
        };
        let asked = flying.wakes.iter().filter_map(|w| match w {
            Wake::Vote { id, .. } => Some(*id),
            _ => None,
        });
        let Some(answer) = result.as_ref().ok().and_then(|r| answer::read(&r.text)) else {
            return Vec::new();
        };
        asked
            .map(|id| {
                let fields = answer.votes.iter().find(|(ruling, _)| *ruling == id);
                let choice = fields
                    .and_then(|(_, f)| crate::panel::from_fields(f, self.panel_options(id)).ok())
                    .map(|vote| vote.choice);
                match self.panel.plan(id) {
                    Some(plan) => plan.mark(id, Voter::Pm, choice),
                    // Its ruling is settled or gone, so only what it was is known.
                    None => VoteMark {
                        ruling: id,
                        ruling_kind: (self.state.rulings.iter().find(|r| r.id == id))
                            .map_or(RulingName::Question, |r| RulingName::of(&r.kind)),
                        voter: Voter::Pm,
                        panel: Tier::Full,
                        work: "untyped".to_owned(),
                        choice,
                    },
                }
            })
            .collect()
    }

    /// Notes what saving `now` changes that wakes the project manager: a
    /// work item parked on a stuck ruling, or one closing, which lets its
    /// holds go
    pub(in crate::runner) fn pm_noticed(&mut self, now: &ProjectState) {
        if self.agents.pm.is_none() {
            return;
        }
        let was = &self.state;
        let raised = (now.rulings.iter()).filter(|r| !was.rulings.iter().any(|w| w.id == r.id));
        let stuck: Vec<Wake> = raised
            .filter_map(|ruling| {
                let issue = ruling.issue?;
                let ended = self.pm.after_end.contains_key(&issue);
                stuck_wake(issue, ruling.id, &ruling.kind).filter(|_| !ended)
            })
            .collect();
        let closed = (was.work_items.iter()).any(|i| now.item(i.issue).is_none());
        let opened = (now.work_items.iter()).any(|i| was.item(i.issue).is_none());
        self.pm_asked(now);
        for wake in stuck {
            self.pm_due(wake);
        }
        // A pick was made on a board that has changed since.
        if (opened || closed) && matches!(self.pm.pick, Some(Pick::Issue(_))) {
            self.pm.pick = None;
        }
        if closed {
            self.pm.held.clear();
            if matches!(self.pm.pick, Some(Pick::Nothing(_))) {
                self.pm.pick = None;
            }
            let open = now.open_issues();
            self.pm.after_end.retain(|issue, _| open.contains(issue));
            (self.pm.conflicts).retain(|(a, b), _| open.contains(a) && open.contains(b));
        }
    }

    // Wakes the project manager for each question ruling `now` raises, and
    // holds the ruling back from the maintainer for its answer, unless it is
    // held by the maintainer's terminal or down, when the ruling goes out as
    // it is. Its pace is checked when the wake starts.
    fn pm_asked(&mut self, now: &ProjectState) {
        let at = self.ports.clock.now();
        let passed = (self.pm.down_until).is_some_and(|until| at < until);
        if passed || now.pm_attached.is_some() {
            return;
        }
        for ruling in &now.rulings {
            let new = !self.state.rulings.iter().any(|r| r.id == ruling.id);
            let (RulingKind::Question { asked, .. }, Some(issue), true) =
                (&ruling.kind, ruling.issue, new)
            else {
                continue;
            };
            self.pm.asking.insert(ruling.id, at);
            self.pm_due(Wake::Question {
                issue,
                id: ruling.id,
                asked: asked.clone(),
            });
        }
    }

    /// Wakes the project manager for each pair of open branches newly in
    /// conflict, given every pair in conflict now
    pub(in crate::runner) fn pm_conflicts(&mut self, now: BTreeMap<(u64, u64), Vec<String>>) {
        if self.agents.pm.is_none() {
            return;
        }
        for (&(a, b), files) in &now {
            if !self.pm.conflicts.contains_key(&(a, b)) {
                self.pm_due(Wake::Conflict(a, b, files.clone()));
            }
        }
        self.pm.conflicts = now;
    }
}

// The wake for `issue`, parked on ruling `id` of `kind`, if that is a stuck item's.
fn stuck_wake(issue: u64, id: u64, kind: &RulingKind) -> Option<Wake> {
    let what = stuck_on(kind)?;
    Some(Wake::Stuck(
        issue,
        format!("{what}, and ruling {id} asks the maintainer"),
    ))
}

/// What a ruling of `kind` says of a stuck work item, or none for one that
/// is not about a stuck item
pub(in crate::runner) fn stuck_on(kind: &RulingKind) -> Option<&'static str> {
    match kind {
        RulingKind::Stuck(Stuck::TurnFailed { .. }) => Some(
            "its worker's turn parked: the call failed once, the model host stayed unreachable for 30 minutes, or the turn stopped short twice",
        ),
        RulingKind::Stuck(Stuck::TurnTimeout { .. }) => {
            Some("its worker's turn ran past its ceiling")
        }
        RulingKind::Stuck(Stuck::StillRed {
            fix_turns: None, ..
        }) => Some("CI failed again and the worker pushed no fix"),
        RulingKind::Stuck(Stuck::StillRed { .. }) => {
            Some("CI failed and the worker's fix turns reached the cap `ci.fix_attempts` sets")
        }
        _ => None,
    }
}
