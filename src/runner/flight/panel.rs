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
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use super::super::Runner;
use super::super::panel::{CEILING, Plan, vote_of};
use super::super::turn_hooks::TurnHooks;
use super::News;
use crate::pacer::Scope;
use crate::panel::Voter;
use crate::ports::{AgentCall, AgentError, AgentReply, Cost, Ending, Timestamp};
use crate::settings::Limit;
use crate::state::StateError;
use crate::usage::{CallKind, CallLine, Draft, Spent, ended};

/// The experts of one ruling, in flight
#[derive(Debug)]
pub(in crate::runner) struct Panelling {
    id: u64,
    options: usize,
    // The work item whose session the worker's vote resumes, while it votes
    worker_of: Option<u64>,
    calls: Vec<Called>,
    drafts: Vec<(Voter, Draft)>,
    // How the ruling's panel is asked, which each vote's ledger line says
    plan: Option<Plan>,
    // How the worker's call ended, once it has, for its ledger line and the
    // cost of its session before the flight as a whole is heard
    worker_result: Arc<Mutex<Option<Result<AgentReply, AgentError>>>>,
    // Whether the ruling was answered, so the work item's session is free of
    // the worker's vote once that call is kept
    released: bool,
    // Whether the worker's ledger line and session cost are already kept
    settled: bool,
}

// One expert's call, and when it is due to be ended: its ceiling counts from
// when it has the lease it waited for, as a worker's turn does, so a call
// queued on a lease is not ended before its model has run.
#[derive(Debug)]
struct Called {
    voter: Voter,
    ending: Ending,
    due: Arc<AtomicU64>,
    // Whether the call has returned
    done: Arc<AtomicBool>,
}

impl Called {
    fn due(&self) -> Timestamp {
        Timestamp(self.due.load(Ordering::SeqCst))
    }
}

impl Panelling {
    // The ruling whose experts are in flight.
    pub(in crate::runner) fn ruling(&self) -> u64 {
        self.id
    }

    // Whether the worker's vote is running in `issue`'s session, which no
    // turn of the work item may resume until it ends.
    pub(in crate::runner) fn votes_as(&self, issue: u64) -> bool {
        let free = self.released && self.settled;
        self.worker_of == Some(issue) && !free
    }

    // Whether the worker's call has returned, or there was none.
    fn worker_done(&self) -> bool {
        (self.calls.iter())
            .filter(|call| call.voter == Voter::Worker)
            .all(|call| call.done.load(Ordering::SeqCst))
    }

    // Ends each call that is past its ceiling.
    pub(super) fn end_overdue(&self, now: Timestamp) {
        (self.calls.iter())
            .filter(|call| call.due() <= now)
            .for_each(|call| call.ending.end());
    }

    // When its calls are due to be ended: the soonest of those not ended.
    pub(super) fn ceiling(&self) -> Option<Timestamp> {
        (self.calls.iter())
            .filter(|call| !call.ending.asked())
            .map(Called::due)
            .min()
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
            panel.calls.iter().for_each(|call| call.ending.end());
        }
    }

    /// Whether the worker's vote on ruling `id` is still to return: its call
    /// runs, or has not been started
    pub(in crate::runner) fn panel_worker_open(&self, id: u64) -> bool {
        match self.flights.panel.as_ref().filter(|p| p.id == id) {
            None => true,
            Some(flight) => (flight.calls.iter())
                .any(|call| call.voter == Voter::Worker && !call.done.load(Ordering::SeqCst)),
        }
    }

    /// Frees the work item of ruling `id` from the worker's vote, now that the
    /// ruling is answered: the call is ended if it runs, and the item's
    /// session is its own again once the call has returned and is kept. The
    /// other experts go on voting. Whether a flight for `id` was there to free.
    pub(in crate::runner) fn panel_release_worker(&mut self, id: u64) -> bool {
        let Some(flight) = self.flights.panel.as_mut().filter(|p| p.id == id) else {
            return false;
        };
        flight.released = true;
        (flight.calls.iter())
            .filter(|call| call.voter == Voter::Worker)
            .for_each(|call| call.ending.end());
        self.panel_settle_worker();
        true
    }

    /// Keeps the ledger line and session cost of the worker's vote once its
    /// call has returned in a flight whose ruling is answered, before the
    /// work item's next turn can count them as its own
    pub(in crate::runner) fn panel_settle_worker(&mut self) {
        let ready = (self.flights.panel.as_ref()).is_some_and(|p| p.released && !p.settled);
        if !ready {
            return;
        }
        let Some(mut flight) = self.flights.panel.take() else {
            return;
        };
        self.settle_worker_of(&mut flight);
        self.flights.panel = Some(flight);
    }

    fn settle_worker_of(&mut self, flight: &mut Panelling) {
        let Some(issue) = flight.worker_of.filter(|_| flight.worker_done()) else {
            return;
        };
        let Some(result) = flight.worker_result.lock().ok().and_then(|r| r.clone()) else {
            return;
        };
        let cost = result.as_ref().ok().and_then(|reply| reply.session_cost);
        if let Some(cost) = cost
            && let Err(e) = self.keep_session_cost(issue, cost)
        {
            self.notes.push(format!(
                "cannot keep the cost of ruling {}'s worker vote: {e}",
                flight.id
            ));
            return;
        }
        let now = self.ports.clock.now();
        if let Some(line) = self.vote_line(flight, Voter::Worker, &result, now) {
            self.append_call(line, false);
        }
        flight.settled = true;
    }

    // The ledger line of `voter`'s vote in `flight`, ended as `result` says.
    fn vote_line(
        &mut self,
        flight: &Panelling,
        voter: Voter,
        result: &Result<AgentReply, AgentError>,
        now: Timestamp,
    ) -> Option<CallLine> {
        let (_, draft) = flight.drafts.iter().find(|(v, _)| *v == voter)?;
        let ended = ended(result)?;
        let spent = result.as_ref().ok().map(Spent::from);
        let before = (draft.session())
            .and_then(|session| self.ledger.session_cost(session))
            .unwrap_or(Cost(0));
        let mut line = draft.line(now, ended, spent, before, self.pacer_lines());
        if let Some(plan) = &flight.plan {
            let choice = vote_of(result, flight.options).ok().map(|v| v.choice);
            line.votes = vec![plan.mark(flight.id, voter, choice)];
        }
        Some(line)
    }

    // The worker's vote ran in its work item's session, whose cost counts
    // up across calls. The vote's own cost is the project's, but the next
    // turn's must not include it, so the item keeps where the session stands.
    fn keep_session_cost(&mut self, issue: u64, cost: Cost) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(item) = next.work_items.iter_mut().find(|i| i.issue == issue) {
            let session = item.session.clone();
            item.advance_session_cost(&session, cost);
            self.save(next)?;
        }
        Ok(())
    }

    /// Asks the experts of the oldest ruling waiting for them, unless a
    /// panel's experts are in flight or the runner drains, and says whether
    /// any started
    pub(in crate::runner) fn panel_ask_next(&mut self) -> bool {
        if self.flights.panel.is_some() || self.draining() {
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
        let Some(ruling) = self.panel_ruling(id) else {
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
        self.launch_panel(id, options, ruling.issue, calls);
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
    fn launch_panel(
        &mut self,
        id: u64,
        options: usize,
        issue: Option<u64>,
        calls: Vec<(Voter, AgentCall, String)>,
    ) {
        let now = self.ports.clock.now();
        let mut drafts = Vec::new();
        let mut called = Vec::new();
        let mut running = Vec::new();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        for (voter, call, agent) in calls {
            let draft = self.with_pull_request(Draft::of(&call, &agent, CallKind::Vote, now));
            self.call_started(&call, &agent);
            drafts.push((voter, draft));
            // Its ceiling starts again once the lease it waited for is its.
            let due = Arc::new(AtomicU64::new(now.0.saturating_add(CEILING)));
            let began = {
                let (due, clock, wake) = (
                    Arc::clone(&due),
                    Arc::clone(&self.ports.clock),
                    wake.clone(),
                );
                move || {
                    due.store(clock.now().0.saturating_add(CEILING), Ordering::SeqCst);
                    if let Some(wake) = &wake {
                        let _ = wake.send(());
                    }
                }
            };
            let ending = Ending::telling(began, |_| {});
            let done = Arc::new(AtomicBool::new(false));
            called.push(Called {
                voter,
                ending: ending.clone(),
                due,
                done: Arc::clone(&done),
            });
            running.push((voter, call, ending, done));
        }
        let calls = called;
        let agents = self.counted(TurnHooks::around(
            Arc::clone(&self.ports.agents),
            &self.hooks,
            &self.notes,
            None,
        ));
        let names: Vec<Voter> = running.iter().map(|(voter, ..)| *voter).collect();
        let worker_result = Arc::new(Mutex::new(None));
        let kept_result = Arc::clone(&worker_result);
        let call_wake = wake.clone();
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                thread::scope(|scope| {
                    let started: Vec<_> = (running.iter())
                        .map(|(voter, call, ending, done)| {
                            let (agents, kept, wake) = (&agents, &kept_result, &call_wake);
                            let ran = move || {
                                let result = agents.run(call, ending);
                                if *voter == Voter::Worker
                                    && let Ok(mut slot) = kept.lock()
                                {
                                    *slot = Some(result.clone());
                                }
                                done.store(true, Ordering::SeqCst);
                                if let Some(wake) = wake {
                                    let _ = wake.send(());
                                }
                                result
                            };
                            (*voter, scope.spawn(ran))
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
        let worker_of = drafts
            .iter()
            .any(|(v, _)| *v == Voter::Worker)
            .then_some(issue)
            .flatten();
        let plan = self.panel.plan(id);
        self.flights.panel = Some(Panelling {
            id,
            options,
            worker_of,
            calls,
            drafts,
            plan,
            worker_result,
            released: false,
            settled: false,
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
            if *voter == Voter::Worker && flight.settled {
                continue;
            }
            if let Some(line) = self.vote_line(&flight, *voter, result, now) {
                self.append_call(line, false);
            }
        }
        // The worker's vote ran in its work item's session, whose cost counts
        // up across calls. The vote's own cost is the project's, but the next
        // turn's must not include it, so the item keeps where the session stands.
        let session_cost = (ends.iter())
            .find(|(voter, _)| *voter == Voter::Worker)
            .and_then(|(_, result)| result.as_ref().ok())
            .and_then(|reply| reply.session_cost);
        if let (Some(issue), Some(cost), false) = (flight.worker_of, session_cost, flight.settled) {
            self.keep_session_cost(issue, cost)?;
        }
        self.panel_heard(flight.id, flight.options, ends);
        debug_assert_eq!(flight.id, id);
        // The next ruling's experts start on the next pass.
        Ok(None)
    }
}
