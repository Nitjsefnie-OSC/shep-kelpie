//! The panel's experts in flight
//!
//! The worker, the advisor and the researcher of one ruling run at once, on
//! a thread each inside one thread of the runner's, so they are blind to each
//! other and none waits on another. One ruling's experts are in flight at a
//! time. Their ends come back together as one piece of news, which records
//! each vote or abstention. The project manager's vote is not here: it is
//! woken as it is for anything else.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::panel::CEILING;
use super::super::turn_hooks::TurnHooks;
use super::News;
use crate::pacer::Scope;
use crate::panel::Voter;
use crate::ports::{AgentCall, AgentError, AgentReply, Cost, Ending, Timestamp};
use crate::settings::Limit;
use crate::state::StateError;
use crate::usage::{CallKind, Draft, Spent, ended};

/// The experts of one ruling, in flight
#[derive(Debug)]
pub(in crate::runner) struct Panelling {
    id: u64,
    options: usize,
    deadline: Timestamp,
    endings: Vec<Ending>,
    drafts: Vec<(Voter, Draft)>,
}

impl Panelling {
    // Ends every call once past the ceiling.
    pub(super) fn end_overdue(&self, now: Timestamp) {
        if self.deadline <= now {
            self.endings.iter().for_each(Ending::end);
        }
    }

    // When its calls are due to be ended, while they have not been.
    pub(super) fn ceiling(&self) -> Option<Timestamp> {
        (self.endings.iter().any(|e| !e.asked())).then_some(self.deadline)
    }
}

impl Runner {
    /// Whether the experts of ruling `id` are in flight
    pub(in crate::runner) fn panel_in_flight(&self, id: u64) -> bool {
        self.flights.panel.as_ref().is_some_and(|p| p.id == id)
    }

    /// Ends every call of the experts in flight; their end comes back as news
    pub(in crate::runner) fn panel_end_flight(&self) {
        if let Some(panel) = &self.flights.panel {
            panel.endings.iter().for_each(Ending::end);
        }
    }

    /// Asks the experts of the oldest ruling waiting for them, unless a
    /// panel's experts are in flight or the runner drains, and says whether
    /// any started
    pub(in crate::runner) fn panel_ask_next(&mut self) -> bool {
        if self.flights.panel.is_some() || self.draining {
            return false;
        }
        while let Some(id) = self.panel.queue_pop(self.ports.clock.now()) {
            if self.ask_experts(id) {
                return true;
            }
        }
        false
    }

    // Makes each expert's call for ruling `id` and starts them, and says
    // whether any started. An expert whose call cannot be made abstains.
    fn ask_experts(&mut self, id: u64) -> bool {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id).cloned() else {
            return false;
        };
        let (brief, options) = super::super::voice::brief(&ruling);
        let wanted = self.panel.waiting(id);
        // Each one's settings deny the other's folder entry by entry, as they
        // are when the settings are written, so both are there by then. The
        // sandbox denies the rest of kelpie's home whole in any case.
        for folder in [&self.paths.advisor, &self.paths.researcher] {
            if let Err(e) = std::fs::create_dir_all(folder) {
                self.notes
                    .push(format!("cannot make {}: {}", folder.display(), e.kind()));
            }
        }
        let mut calls: Vec<(Voter, AgentCall, String)> = Vec::new();
        let mut worker_text = None;
        if wanted.contains(&Voter::Worker) {
            let limit = (ruling.issue.and_then(|n| self.state.item(n)))
                .and_then(|item| self.worker_agent(item).ok())
                .map(|agent| agent.limit);
            let made = self
                .not_held(limit.as_ref())
                .and_then(|()| self.worker_vote_call(&ruling, &brief));
            match made {
                Ok(call) => {
                    worker_text = self.worker_session_text(&call);
                    let name = self.ledger_worker(ruling.issue.unwrap_or(0));
                    calls.push((Voter::Worker, call, name));
                }
                Err(why) => self.panel_abstains(id, Voter::Worker, why),
            }
        }
        if wanted.contains(&Voter::Advisor) {
            let agent = self.agents.advisor.clone();
            let made = self
                .not_held(agent.as_ref().map(|a| &a.limit))
                .and_then(|()| self.advisor_call(&ruling, &brief));
            match (made, agent) {
                (Ok(call), Some(agent)) => {
                    calls.push((Voter::Advisor, call, agent.name.as_str().to_owned()));
                }
                (Err(why), _) => self.panel_abstains(id, Voter::Advisor, why),
                (Ok(_), None) => {}
            }
        }
        if wanted.contains(&Voter::Researcher) {
            let agent = self.agents.researcher.clone();
            let made = self
                .not_held(agent.as_ref().map(|a| &a.limit))
                .and_then(|()| self.researcher_call(&ruling, &brief, worker_text.clone()));
            match (made, agent) {
                (Ok(call), Some(agent)) => {
                    calls.push((Voter::Researcher, call, agent.name.as_str().to_owned()));
                }
                (Err(why), _) => self.panel_abstains(id, Voter::Researcher, why),
                (Ok(_), None) => {}
            }
        }
        if calls.is_empty() {
            return false;
        }
        self.launch_panel(id, options, calls);
        true
    }

    // Whether the pacer lets a call on `limit` start, or why not.
    fn not_held(&mut self, limit: Option<&Limit>) -> Result<(), String> {
        let Some(limit) = limit else {
            return Ok(());
        };
        match self.pace(Scope::Turn, limit) {
            Ok(super::super::pace::Pace::Clear) => Ok(()),
            Ok(_) => Err("the pacer holds its account".to_owned()),
            Err(e) => Err(format!("its account's usage cannot be paced: {e}")),
        }
    }

    // Starts `calls` at once on threads of their own, in one thread of the
    // runner's whose end comes back as news.
    fn launch_panel(&mut self, id: u64, options: usize, calls: Vec<(Voter, AgentCall, String)>) {
        let now = self.ports.clock.now();
        let mut drafts = Vec::new();
        let mut endings = Vec::new();
        let mut running = Vec::new();
        for (voter, call, agent) in calls {
            let draft = self.with_pull_request(Draft::of(&call, &agent, CallKind::Vote, now));
            self.call_started(&call, &agent);
            drafts.push((voter, draft));
            let ending = Ending::default();
            endings.push(ending.clone());
            running.push((voter, call, ending));
        }
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let agents = TurnHooks::around(
            Arc::clone(&self.ports.agents),
            &self.hooks,
            &self.notes,
            None,
        );
        let names: Vec<Voter> = running.iter().map(|(voter, ..)| *voter).collect();
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                thread::scope(|scope| {
                    let started: Vec<_> = (running.iter())
                        .map(|(voter, call, ending)| {
                            let agents = &agents;
                            (*voter, scope.spawn(move || agents.run(call, ending)))
                        })
                        .collect();
                    started
                        .into_iter()
                        .map(|(voter, handle)| {
                            let result = handle.join().unwrap_or_else(|_| {
                                Err(AgentError::Setup("its call panicked".to_owned()))
                            });
                            (voter, result)
                        })
                        .collect::<Vec<_>>()
                })
            }));
            let ends = ran.unwrap_or_else(|_| {
                (names.iter())
                    .map(|&voter| (voter, Err(AgentError::Setup("its calls panicked".into()))))
                    .collect()
            });
            let _ = news.send(News::Paneled { id, ends });
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        if let Err(e) = thread::Builder::new()
            .name(format!("ruling {id} panel"))
            .spawn(run)
        {
            let reason = format!("cannot start a thread for the calls: {e}");
            let ends = (drafts.iter())
                .map(|(voter, _)| (*voter, Err(AgentError::Setup(reason.clone()))))
                .collect();
            let _ = self.flights.send.send(News::Paneled { id, ends });
        }
        self.flights.panel = Some(Panelling {
            id,
            options,
            deadline: Timestamp(now.0.saturating_add(CEILING)),
            endings,
            drafts,
        });
    }

    // Records the end of the experts' calls: each one's line in the ledger,
    // and each vote or abstention on the ruling.
    pub(super) fn panel_ended(
        &mut self,
        id: u64,
        ends: Vec<(Voter, Result<AgentReply, AgentError>)>,
    ) -> Result<Option<crate::runner::StepReport>, StateError> {
        let Some(flight) = self.flights.panel.take() else {
            return Ok(None);
        };
        let now = self.ports.clock.now();
        for (voter, result) in &ends {
            let Some((_, draft)) = flight.drafts.iter().find(|(v, _)| v == voter) else {
                continue;
            };
            let Some(ended) = ended(result) else {
                continue;
            };
            let spent = result.as_ref().ok().map(Spent::from);
            let before = (draft.session())
                .and_then(|session| self.ledger.session_cost(session))
                .unwrap_or(Cost(0));
            let line = draft.line(now, ended, spent, before, self.pacer_lines());
            self.append_call(line, false);
        }
        self.panel_heard(flight.id, flight.options, ends);
        debug_assert_eq!(flight.id, id);
        // The next ruling's experts start on the next pass.
        Ok(None)
    }
}
