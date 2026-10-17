//! Calls in flight
//!
//! The runner starts each agent call on a thread of its own and goes on, so
//! no thread of the runner's waits on one. The call's end comes back on a
//! channel, which wakes the runner's loop, and its next pass records it. A
//! work item has at most one call in flight, and the open work items' calls
//! run at once, beside the project manager's one call. A turn's ceiling is
//! a deadline each pass checks, and a call past it is ended through its own
//! process group.

use std::any::Any;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::Runner;
use super::alert::post_due;
use super::briefing::{Watched, brief};
use super::dev_server;
use super::in_flight::InFlight;
use super::login_refresh;
use super::replies::answer_replies;
use super::report::{Begin, ReviewCall, ReviewResult, Reviewed, Spent, StepReport};
use super::review::run_review_call;
use super::transcript_count::Mark;
use super::trigger::lock;
use super::turn_hooks::TurnHooks;
use crate::adapters::bridge::Bridge;
use crate::board::briefing::elapsed;
use crate::ports::{
    AgentCall, AgentError, AgentReply, Ending, Flying, RoundStage, Timestamp, Wait,
};
use crate::settings::Harness;
use crate::state::StateError;
use crate::usage::{CallKind, Draft};

/// What one pass of the runner's loop did
#[derive(Debug)]
pub enum Pass {
    /// Nothing was due
    Idle,
    /// Something happened, for the runner's log
    Report(StepReport),
    /// A call started, and goes on in flight
    Started,
}

/// The runner's calls in flight, and the channel their ends come back on
#[derive(Debug)]
pub(super) struct Flights {
    flying: BTreeMap<u64, Flight>,
    // The project manager's call, its ceiling and its hold
    pm: Option<(Timestamp, Ending)>,
    // The issue writer's call labelling an issue
    labelling: Option<label::Labelling>,
    // The security review of a contribution's head
    securing: Option<security::Securing>,
    // The triager's research of an untouched issue
    triaging: Option<triage::Triaging>,
    // The e2e run of a bump's head
    proving: proving::Proving,
    // The contributions a security review started on since the last poll, which
    // are not tried again before the next
    security_tried: Vec<u64>,
    // The issues triage looked at since the last poll
    triage_tried: Vec<u64>,
    // The experts of one ruling, voting
    panel: Option<panel::Panelling>,
    // The project manager's batch retro
    retro: Option<retro::Retrospecting>,
    // Every call's line in the usage ledger, as it started
    ledger: InFlight,
    send: Sender<News>,
    news: Receiver<News>,
    // Told after each piece of news, so the loop waiting on it wakes
    wake: Option<Sender<()>>,
}

impl Default for Flights {
    fn default() -> Self {
        let (send, news) = mpsc::channel();
        Self {
            flying: BTreeMap::new(),
            pm: None,
            labelling: None,
            securing: None,
            triaging: None,
            proving: proving::Proving::default(),
            security_tried: Vec::new(),
            triage_tried: Vec::new(),
            panel: None,
            retro: None,
            ledger: InFlight::default(),
            send,
            news,
            wake: None,
        }
    }
}

impl Flights {
    /// The issue the issue writer is labelling, while it is
    pub(super) fn labelling(&self) -> Option<u64> {
        self.labelling.as_ref().map(|l| l.issue)
    }

    /// Whether `issue`'s work item has a call in flight
    /// A worker's vote on a ruling counts, since it runs in the work item's session
    pub(super) fn flying(&self, issue: u64) -> bool {
        self.flying.contains_key(&issue) || self.panel.iter().any(|p| p.votes_as(issue))
    }

    /// The ruling whose panel's experts are in flight, while they are
    pub(super) fn panel_ruling(&self) -> Option<u64> {
        self.panel.as_ref().map(panel::Panelling::ruling)
    }

    /// Whether `issue`'s work item has a worker's turn in flight
    ///
    /// A turn marked running in the state file is not enough to call its
    /// time the worker's: a stop leaves one marked, and so does a restart
    /// until a pass resumes it.
    pub(super) fn runs_turn(&self, issue: u64) -> bool {
        self.flying
            .get(&issue)
            .is_some_and(|f| f.deadline.is_some())
    }

    /// Forgets `issue`'s call in flight, once its end is heard
    pub(super) fn landed(&mut self, issue: u64) {
        self.flying.remove(&issue);
    }

    /// The call of `issue`'s worker turn in flight, whether it is its work
    /// item's first, and the seconds of its ceiling it began with
    pub(super) fn turn_call(&self, issue: u64) -> Option<(AgentCall, bool, u64)> {
        let flight = self.flying.get(&issue)?;
        let call = flight.watched.call.clone()?;
        (flight.deadline.is_some() && !flight.compact).then_some((call, flight.first, flight.left))
    }

    /// When `issue`'s call in flight started
    pub(super) fn started(&self, issue: u64) -> Option<Timestamp> {
        Some(self.flying.get(&issue)?.watched.started)
    }

    /// Whether `issue`'s call in flight is a `/compact` of its worker's session
    pub(super) fn runs_compact(&self, issue: u64) -> bool {
        self.flying.get(&issue).is_some_and(|f| f.compact)
    }

    /// Whether the panel's experts are in flight
    pub(super) fn panel_running(&self) -> bool {
        self.panel.is_some()
    }

    /// Whether the batch retro is in flight
    pub(super) fn retro_running(&self) -> bool {
        self.retro.is_some()
    }

    /// Ends `issue`'s call in flight, as its ceiling would
    pub(super) fn end(&self, issue: u64) {
        if let Some(flight) = self.flying.get(&issue) {
            flight.ending.end();
        }
    }

    /// The calls in flight as the usage ledger knows them
    pub(super) fn in_flight(&self) -> InFlight {
        self.ledger.clone()
    }

    /// `issue`'s call in flight, as the board watches it
    pub(super) fn watched(&self, issue: u64) -> Option<&Watched> {
        self.flying.get(&issue).map(|f| &f.watched)
    }

    /// Every call in flight by its work item's issue, as the board watches it
    pub(super) fn watched_mut(&mut self) -> impl Iterator<Item = (u64, &mut Watched)> {
        self.flying
            .iter_mut()
            .map(|(&issue, f)| (issue, &mut f.watched))
    }
}

// One call in flight
#[derive(Debug)]
struct Flight {
    // The turn's ceiling for a worker's turn, and none for a review call
    deadline: Option<Timestamp>,
    // How much of the ceiling the turn had when it started, which it has
    // again from when it gets the lease it waited for
    left: u64,
    ending: Ending,
    // Whether this call starts over a session that died before it began
    again: bool,
    watched: Watched,
    // Whether a worker's turn is its work item's first, with no session to keep
    first: bool,
    // Whether it is a `/compact` of the worker's session, sent before a turn
    compact: bool,
    // Where a first turn that waited too long moves
    fallback: waiting::Fallback,
    // The browser the turn drives, which lasts as long as the turn does
    browser: Option<Bridge>,
    // Where the turn's transcript stood as it began, for a handover to carry
    requests: Option<Mark>,
}

impl Flight {
    // What the call's record keeps of this flight, counting from `began`
    fn flying(&self, began: Timestamp) -> Flying {
        Flying {
            left: self.left,
            began,
            again: self.again,
            first: self.first,
            compact: self.compact,
        }
    }
}

// What a call's thread sends the runner
enum News {
    Ended {
        issue: u64,
        end: End,
    },
    // The project manager's call ended
    Pm(End),
    // The batch retro's call ended
    Retro(End),
    // The issue writer's call labelling an issue ended
    Labelled {
        issue: u64,
        end: End,
    },
    // The e2e run of a bump's head ended
    Proved(proving::Proof),
    // A contribution's head was read for security, to a verdict or to none
    Secured {
        pull_request: u64,
        head: String,
        reading: crate::runner::contributions::Reading,
    },
    // The triager's research of an issue ended, with its reply read against its
    // view. The reply is boxed: it is the largest thing any `News` variant holds
    // by far, and the rest of the enum is small next to it.
    Triaged {
        issue: u64,
        end: End,
        found: Option<Box<Result<crate::runner::triage::research::Found, String>>>,
    },
    // The experts of a ruling ended, each as its own call did
    Paneled {
        id: u64,
        ends: Vec<(crate::panel::Voter, Result<AgentReply, AgentError>)>,
    },
    // A local round queued for the GPU or began to run. The call waits until
    // `_seen` drops, once the stage is saved, so its time is charged right.
    Stage {
        issue: u64,
        stage: RoundStage,
        _seen: Sender<()>,
    },
    // The call has the lease it waited for, and begins now. It waits until
    // `_seen` drops, once its ceiling counts from now.
    Began {
        issue: u64,
        _seen: Sender<()>,
    },
    // The call waits for its model
    Waits {
        issue: u64,
        wait: Wait,
    },
}

// How a call ended
enum End {
    Turn(Result<AgentReply, AgentError>),
    Review(Reviewed),
    // The call's thread panicked, which ends the runner as a panic in its
    // own thread does
    Panicked(Box<dyn Any + Send>),
}

impl End {
    // Whether it ended before reaching a model: its settings, its harness or
    // its session never got that far
    fn reached_no_model(&self) -> bool {
        match self {
            Self::Turn(result) => crate::usage::ended(result).is_none(),
            Self::Review(reviewed) => matches!(
                (&reviewed.result, &reviewed.spent),
                (ReviewResult::Findings(Err(_)), None)
            ),
            Self::Panicked(_) => false,
        }
    }
}

// What a call's thread needs to tell the runner about its call
#[derive(Clone)]
struct Tell {
    issue: u64,
    news: Sender<News>,
    wake: Option<Sender<()>>,
}

impl Tell {
    fn send(&self, news: News) -> bool {
        let sent = self.news.send(news).is_ok();
        if let Some(wake) = &self.wake {
            let _ = wake.send(());
        }
        sent
    }

    fn stage(&self, stage: RoundStage) {
        let issue = self.issue;
        self.until_heard(|seen| News::Stage {
            issue,
            stage,
            _seen: seen,
        });
    }

    fn began(&self) {
        let issue = self.issue;
        self.until_heard(|seen| News::Began { issue, _seen: seen });
    }

    fn waits(&self, wait: Wait) {
        let issue = self.issue;
        self.send(News::Waits { issue, wait });
    }

    // Sends the news `news` makes, and waits until the runner has heard it.
    fn until_heard(&self, news: impl FnOnce(Sender<()>) -> News) {
        let (seen, heard) = mpsc::channel();
        if self.send(news(seen)) {
            let _ = heard.recv();
        }
    }
}

// A call to start, as a step found it
enum Launch {
    Turn {
        call: AgentCall,
        deadline: Timestamp,
        again: bool,
        first: bool,
        compact: bool,
    },
    Review(ReviewCall),
}

impl Runner {
    /// Wakes `wake` whenever a call in flight has news, as each call started
    /// from now on does
    pub fn wake_with(&mut self, wake: Sender<()>) {
        self.flights.wake = Some(wake);
    }

    /// How long until the soonest ceiling of a turn in flight that has not
    /// been ended yet, or `None` with no such turn
    pub fn next_ceiling(&self) -> Option<Duration> {
        let now = self.ports.clock.now();
        let turns = (self.flights.flying.values())
            .filter(|flight| !flight.ending.asked())
            .filter_map(|flight| flight.deadline)
            .chain(self.fallback_due());
        let pm = (self.flights.pm.iter())
            .filter(|(_, ending)| !ending.asked())
            .map(|(deadline, _)| *deadline);
        let labelling = (self.flights.labelling.iter()).filter_map(label::Labelling::ceiling);
        let securing = (self.flights.securing.iter()).filter_map(security::Securing::ceiling);
        let triaging = (self.flights.triaging.iter()).filter_map(triage::Triaging::ceiling);
        let panel = (self.flights.panel.iter()).filter_map(panel::Panelling::ceiling);
        let retro = (self.flights.retro.iter()).filter_map(retro::Retrospecting::ceiling);
        turns
            .chain(pm)
            .chain(labelling)
            .chain(securing)
            .chain(triaging)
            .chain(panel)
            .chain(retro)
            .map(|deadline| Duration::from_secs(deadline.0.saturating_sub(now.0)))
            .min()
    }

    // Ends each call in flight past its turn's ceiling. Its end comes back
    // as a call that timed out, and parks as one.
    fn end_overdue(&self) {
        let now = self.ports.clock.now();
        for flight in self.flights.flying.values() {
            if flight.deadline.is_some_and(|deadline| deadline <= now) {
                flight.ending.end();
            }
        }
        if let Some((deadline, ending)) = &self.flights.pm
            && *deadline <= now
        {
            ending.end();
        }
        if let Some(labelling) = &self.flights.labelling {
            labelling.end_overdue(now);
        }
        if let Some(securing) = &self.flights.securing {
            securing.end_overdue(now);
        }
        if let Some(triaging) = &self.flights.triaging {
            triaging.end_overdue(now);
        }
        if let Some(panel) = &self.flights.panel {
            panel.end_overdue(now);
        }
        if let Some(retro) = &self.flights.retro {
            retro.end_overdue(now);
        }
    }

    /// Starts the project manager's `call`, a `kind` on the agent named
    /// `agent`, on a thread of its own, ended past `ceiling` seconds, whose
    /// end comes back as news
    pub(super) fn launch_pm(&mut self, call: AgentCall, ceiling: u64, agent: &str, kind: CallKind) {
        let now = self.ports.clock.now();
        let draft = Draft::of(&call, agent, kind, now);
        self.flights.ledger.open(None, draft, self.pacer_lines());
        self.call_started(&call, agent);
        let ledger = self.flights.ledger.clone();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let ending = Ending::default();
        ending.carry(Flying {
            left: ceiling,
            began: now,
            again: false,
            first: false,
            compact: false,
        });
        let held = ending.clone();
        let agents = self.counted(TurnHooks::around(
            Arc::clone(&self.ports.agents),
            &self.hooks,
            &self.notes,
            None,
        ));
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| End::Turn(agents.run(&call, &held))));
            let end = ran.unwrap_or_else(End::Panicked);
            if end.reached_no_model() {
                ledger.no_model(None);
            }
            let _ = news.send(News::Pm(end));
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        if let Err(e) = thread::Builder::new().name("pm".into()).spawn(run) {
            let reason = format!("cannot start a thread for the call: {e}");
            let end = End::Turn(Err(AgentError::Spawn(Harness::ClaudeCode, reason)));
            let _ = self.flights.send.send(News::Pm(end));
        }
        self.flights.pm = Some((Timestamp(now.0.saturating_add(ceiling)), ending));
    }

    // Counts `issue`'s turn ceiling from now, once its call has the lease it
    // waited for, and starts the board's quiet clock from the same moment.
    fn began(&mut self, issue: u64) {
        let now = self.ports.clock.now();
        let told = self.flights.flying.get_mut(&issue).map(|flight| {
            let told = flight.watched.told.take();
            let said = flight.watched.woken;
            let turn = flight.watched.call.is_some();
            let waited = told.map_or(0, |(since, _)| now.0.saturating_sub(since.0));
            flight.watched.granted = Some(now);
            flight.watched.woken = false;
            if flight.deadline.is_some() {
                flight.deadline = Some(Timestamp(now.0.saturating_add(flight.left)));
                flight.ending.carry(flight.flying(now));
            }
            (said, turn, waited)
        });
        // The board names the gap the lease took, so a reader sees the wait was real.
        if let Some((said, turn, waited)) = told
            && said
        {
            let who = if turn { "worker" } else { "review call" };
            self.board_event(format!(
                "#{issue}: {who} has its model's lease, after waiting {}",
                elapsed(waited)
            ));
        }
        self.board_changed();
    }

    // Begins what is due next, starting the call it names. `start_over` is
    // the work item whose session died unborn, which begins its turn again.
    fn begin(&mut self, start_over: Option<u64>) -> Result<Pass, StateError> {
        let begin = self.begin_turn(start_over)?;
        let launch = match begin {
            Begin::Idle => return Ok(Pass::Idle),
            Begin::Report(report) => return Ok(Pass::Report(report)),
            Begin::Call {
                call,
                deadline,
                first,
                compact,
            } => Launch::Turn {
                call,
                deadline,
                again: start_over.is_some(),
                first,
                compact,
            },
            Begin::Review(action) => Launch::Review(action),
            Begin::Label(issue) => {
                let report = self.ask_writer(&issue)?;
                // The issue writer's call is in flight from here, and the pass
                // goes on holding the runner's lock.
                self.post();
                return Ok(report.map_or(Pass::Started, Pass::Report));
            }
            Begin::Secure(job) => {
                self.launch_security(job);
                self.post();
                return Ok(Pass::Started);
            }
            Begin::Triage(job) => {
                self.launch_triage(job);
                self.post();
                return Ok(Pass::Started);
            }
        };
        let issue = self.focus.expect("a call is a work item's");
        self.launch(issue, launch);
        // The call is in flight from here, and the pass goes on with the lock
        // held. A trigger refused it answers from this post, so `drain` never
        // answers that nothing runs while a call has just started.
        self.post();
        Ok(Pass::Started)
    }

    // Starts `launch` on a thread of its own, whose end comes back as news.
    fn launch(&mut self, issue: u64, launch: Launch) {
        let tell = Tell {
            issue,
            news: self.flights.send.clone(),
            wake: self.flights.wake.clone(),
        };
        let ending = Ending::telling(
            {
                let tell = tell.clone();
                move || tell.began()
            },
            {
                let tell = tell.clone();
                move |wait| tell.waits(wait)
            },
        );
        // A turn's harness, which a turn that cannot start names
        let (deadline, again, first, harness) = match &launch {
            Launch::Turn {
                call,
                deadline,
                again,
                first,
                ..
            } => (
                Some(*deadline),
                *again,
                *first,
                Some(call.harness.harness()),
            ),
            Launch::Review(_) => (None, false, false, None),
        };
        let compact = matches!(launch, Launch::Turn { compact: true, .. });
        let role = harness.map_or("reviewer", |_| "worker");
        let now = self.ports.clock.now();
        let watched = Watched {
            started: now,
            call: match &launch {
                Launch::Turn { call, .. } => Some(call.clone()),
                Launch::Review(_) => None,
            },
            idle: false,
            told: None,
            granted: None,
            woken: false,
            lease: waiting::lease_of(&launch),
        };
        let draft = match &launch {
            Launch::Turn { call, .. } => {
                let kind = match compact {
                    true => CallKind::Compact,
                    false => CallKind::Turn,
                };
                Draft::of(call, &self.ledger_worker(issue), kind, now)
            }
            Launch::Review(ReviewCall::Session(call)) => {
                Draft::of(call, &self.ledger_reviewer(issue), CallKind::Review, now)
            }
            Launch::Review(ReviewCall::Local { local, .. }) => {
                Draft::local(local, issue, &self.ledger_reviewer(issue), now)
            }
        };
        let draft = self.with_pull_request(draft);
        self.flights
            .ledger
            .open(Some(issue), draft, self.pacer_lines());
        match &launch {
            Launch::Turn { call, .. } => self.call_started(call, &self.ledger_worker(issue)),
            Launch::Review(ReviewCall::Session(call)) => {
                self.call_started(call, &self.ledger_reviewer(issue));
            }
            Launch::Review(ReviewCall::Local { .. }) => {}
        }
        let ledger = self.flights.ledger.clone();
        let item = self.state.work_items.iter().find(|i| i.issue == issue);
        let agents = self.counted(TurnHooks::around(
            Arc::clone(&self.ports.agents),
            &self.hooks,
            &self.notes,
            item,
        ));
        let reviewer = Arc::clone(&self.ports.reviewer);
        let left = deadline.map_or(0, |deadline| deadline.0.saturating_sub(now.0));
        let flight = Flight {
            deadline,
            left,
            ending,
            again,
            watched,
            first,
            compact,
            fallback: waiting::Fallback::Stays,
            browser: deadline.and_then(|_| self.opened.take(issue)),
            requests: match &launch {
                Launch::Turn { call, .. } => Mark::of(self.ports.agents.as_ref(), call),
                Launch::Review(_) => None,
            },
        };
        // Only a turn has a ceiling to carry, for its record on disk.
        if flight.deadline.is_some() {
            flight.ending.carry(flight.flying(now));
        }
        let held = flight.ending.clone();
        let (voice, remote) = (Arc::clone(&self.ports.voice), self.remote.clone());
        let (notes, metrics) = (self.notes.clone(), Arc::clone(&self.metrics));
        let servers = Arc::clone(&self.ports.dev_servers);
        let dev = match &launch {
            Launch::Turn { call, .. } => self.dev_run(call),
            Launch::Review(_) => None,
        };
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| match launch {
                // A covered worker's token lasts an hour, a call may not.
                Launch::Turn { mut call, .. } => {
                    // Here, with the runner's lock let go, since an index takes a while.
                    super::code_graph::indexed(&mut call, &notes, &metrics, &held);
                    // Started here too, and stopped when the call returns, however it ends.
                    let _server = (dev.as_ref()).and_then(|run| {
                        dev_server::started(servers.as_ref(), run, &mut call, &notes, &metrics)
                    });
                    let build = login_refresh::covered_build(&call);
                    End::Turn(login_refresh::while_running(
                        voice.as_ref(),
                        &remote,
                        build.as_deref(),
                        login_refresh::EVERY,
                        || agents.run(&call, &held),
                    ))
                }
                Launch::Review(mut action) => {
                    if let ReviewCall::Session(call) = &mut action {
                        super::code_graph::indexed(call, &notes, &metrics, &held);
                    }
                    let watch = |stage| tell.stage(stage);
                    let reviewed =
                        run_review_call(agents.as_ref(), reviewer.as_ref(), action, &held, &watch);
                    End::Review(reviewed)
                }
            }));
            let end = ran.unwrap_or_else(End::Panicked);
            if end.reached_no_model() {
                ledger.no_model(Some(issue));
            }
            tell.send(News::Ended { issue, end });
        };
        let started = thread::Builder::new()
            .name(format!("#{issue} {role}"))
            .spawn(run);
        if let Err(e) = started {
            let end = unstarted(harness, &e);
            let _ = self.flights.send.send(News::Ended { issue, end });
        }
        self.flights.flying.insert(issue, flight);
        self.board_changed();
    }

    // Records the end of `issue`'s turn. A session that died before it
    // began starts over once, at once, with the same id.
    fn turn_ended(
        &mut self,
        issue: u64,
        result: Result<AgentReply, AgentError>,
    ) -> Result<Option<StepReport>, StateError> {
        // A host that cannot be reached holds the turn back, to be tried
        // again, and keeps its session. Time spent on the call is saved
        // while its flight still counts it.
        if self.waits_for_host(issue, &result) {
            let saved = self.save_time();
            self.flights.flying.remove(&issue);
            saved?;
            return Ok(None);
        }
        let again = self.flights.flying.get(&issue).is_some_and(|f| f.again);
        if !again && matches!(result, Err(AgentError::NoSession(..))) {
            // The dead call's time is the worker's, so it is saved while its
            // flight still counts it, before a drain can outrun the new turn.
            let saved = self.save_time();
            self.flights.flying.remove(&issue);
            saved?;
            return match self.begin(Some(issue))? {
                Pass::Report(report) => Ok(Some(report)),
                Pass::Idle | Pass::Started => Ok(None),
            };
        }
        if let Some(moved) = self.fell_back(issue, &result) {
            self.flights.flying.remove(&issue);
            return moved;
        }
        let ended = self.on(Some(issue)).end_turn(result);
        self.flights.flying.remove(&issue);
        ended
    }

    fn next_news(&self) -> Option<News> {
        self.flights.news.try_recv().ok()
    }
}

// The end of a call whose thread could not start: a turn on `harness`, or a
// review call.
fn unstarted(harness: Option<Harness>, e: &std::io::Error) -> End {
    let reason = format!("cannot start a thread for the call: {e}");
    match harness {
        Some(harness) => End::Turn(Err(AgentError::Spawn(harness, reason))),
        None => End::Review(Reviewed::failed(reason)),
    }
}

/// Runs one pass of the runner's loop
///
/// Records the end of a call in flight, ends any call past its turn's
/// ceiling, posts a ruling or a notice, handles a reply on the webhook's
/// topic, or begins what is due next for a work item with no call in
/// flight, starting its call. Each open work item is stepped in turn,
/// starting after the one that did something last, and one with nothing to
/// do yields to the next. A report that waits, such as a forge that cannot
/// be read, is returned only when no other work item did anything. A
/// ruling answered while the runner was stopped is acted on first. A ruling
/// is posted, and a reply handled, while the runner drains too. A runner
/// finishing with no work item left open reports that it finished.
///
/// # Errors
///
/// [`StateError`] when a call's start or end, or a post, cannot be saved.
///
/// # Panics
///
/// Resumes the panic of a call's thread, as one in the runner's own would be.
#[track_caller]
pub fn advance(runner: &Mutex<Runner>) -> Result<Pass, StateError> {
    // A pass holds the runner's lock through its forge calls, and each of those
    // waits its turn behind the login's pace. Post what `status` and `drain`
    // read before the pass asks the forge anything, and again once it has
    // started or ended a call.
    lock(runner).post();
    let pass = one_pass(runner);
    if let Ok(Pass::Report(report)) = &pass {
        lock(runner).journal_report(report);
    }
    // After the pass, so a ruling it raised is told in the pass that raised it.
    lock(runner).tell_rulings();
    brief(runner);
    // The project manager wakes only on a pass that started nothing else,
    // once the board it reads is written.
    let pass = match pass {
        // Experts that just started, or a wake, are calls in flight: the loop goes on.
        Ok(Pass::Idle) => {
            let woke = lock(runner).wake_pm();
            let asked = lock(runner).panel_launched();
            let looked = !woke && lock(runner).retro_launched();
            let proved = lock(runner).proofs_launched();
            Ok(if woke || asked || looked || proved {
                Pass::Started
            } else {
                Pass::Idle
            })
        }
        Ok(Pass::Report(report)) if report.waits() => {
            lock(runner).wake_pm();
            Ok(Pass::Report(report))
        }
        pass => pass,
    };
    lock(runner).post();
    pass
}

#[track_caller]
fn one_pass(runner: &Mutex<Runner>) -> Result<Pass, StateError> {
    // A `drain` or `undrain` that landed while a pass held the lock set its
    // flag at once; what it asked the runner itself is taken up here.
    lock(runner).take_posted();
    lock(runner).beat();
    lock(runner).send_metrics();
    lock(runner).let_go_of_the_gone();
    // Before any answer is taken, so none resumes a worker on a closed issue.
    lock(runner).read_parked()?;
    // The forge is asked with the lock let go.
    super::thread_replies::read_threads(runner)?;
    super::left::answer_left(runner);
    loop {
        let news = lock(runner).next_news();
        let Some(news) = news else { break };
        if let Some(report) = hear(runner, news)? {
            return Ok(Pass::Report(report));
        }
    }
    lock(runner).end_overdue();
    super::builds::watch(runner);
    // Before any turn is chosen, so none resumes a session a vote still counts in.
    lock(runner).panel_settle_worker();
    lock(runner).fall_back();
    let alerts = Arc::clone(&lock(runner).ports.alerts);
    if let Some(posted) = post_due(runner, alerts.as_ref()) {
        return posted.map(Pass::Report);
    }
    if let Some(answered) = answer_replies(runner, alerts.as_ref()) {
        return answered.map(Pass::Report);
    }
    // After the posts, so a merge's notice goes before the word it finished.
    let finished = lock(runner).finished_now()?;
    if let Some(report) = finished {
        return Ok(Pass::Report(report));
    }
    lock(runner).begin(None)
}

// Records one piece of news from a call in flight, and tells the project's `ruling`
// hooks the ruling it may have raised.
fn hear(runner: &Mutex<Runner>, news: News) -> Result<Option<StepReport>, StateError> {
    let heard = hear_news(runner, news);
    lock(runner).tell_rulings();
    // A call's end is what `drain` answers with, and the pass goes on to ask
    // the forge with the lock held.
    lock(runner).post();
    heard
}

fn hear_news(runner: &Mutex<Runner>, news: News) -> Result<Option<StepReport>, StateError> {
    let mut runner = lock(runner);
    let (issue, end) = match news {
        News::Stage { issue, stage, .. } => {
            if stage == RoundStage::Running {
                let now = runner.ports.clock.now();
                runner.flights.ledger.ran(Some(issue), now);
                // The round has the lease it queued for.
                runner.began(issue);
            } else {
                // A round queued at its model's lease waits like a turn does.
                runner.told(issue, Wait::Lease);
            }
            // A failed save is told and let go. A GPU wait may then count as
            // the round's, but the phases still sum to the wall time.
            if let Err(e) = runner.on(Some(issue)).round_stage(stage) {
                eprintln!("cannot save the local round's stage: {e}");
            }
            return Ok(None);
        }
        News::Began { issue, .. } => {
            runner.began(issue);
            return Ok(None);
        }
        News::Waits { issue, wait } => {
            runner.told(issue, wait);
            return Ok(None);
        }
        News::Labelled { issue, end } => return runner.label_ended(issue, end),
        News::Secured {
            pull_request,
            head,
            reading,
        } => {
            runner.security_heard(pull_request, &head, reading)?;
            return Ok(None);
        }
        News::Triaged { issue, end, found } => {
            runner.triage_heard(issue, end, found.map(|found| *found))?;
            return Ok(None);
        }
        News::Proved(proof) => {
            runner.proof_heard(proof)?;
            return Ok(None);
        }
        News::Paneled { id, ends } => return runner.panel_ended(id, ends),
        News::Retro(end) => {
            runner.retro_heard(end);
            return Ok(None);
        }
        News::Ended { issue, end } => (issue, end),
        News::Pm(end) => {
            runner.flights.pm = None;
            let open = runner.flights.ledger.take(None);
            return match end {
                End::Turn(result) => {
                    if let Some(open) = &open {
                        runner.plain_line(open, &result);
                    }
                    runner.pm_ended(result)
                }
                End::Review(_) => Ok(None),
                End::Panicked(panic) => {
                    if let Some(open) = &open {
                        runner.panicked_line(open);
                    }
                    drop(runner);
                    resume_unwind(panic)
                }
            };
        }
    };
    let open = runner.flights.ledger.take(Some(issue));
    // A call's line goes in only once its end is saved: one not saved runs
    // again, and its session's cost by then takes in this call's.
    match end {
        End::Turn(result) if runner.flights.runs_compact(issue) => {
            let line = open.and_then(|open| runner.turn_line(&open, &result));
            let unreported = result.is_err();
            let ended = runner.on(Some(issue)).compact_ended(result);
            if let (Some(line), Ok(_)) = (line, &ended) {
                runner.append_call(line, unreported);
            }
            ended
        }
        End::Turn(result) => {
            let line = open.and_then(|open| runner.turn_line(&open, &result));
            let unreported = result.is_err();
            let ended = runner.turn_ended(issue, result);
            if let (Some(line), Ok(_)) = (line, &ended) {
                runner.append_call(line, unreported);
            }
            runner.pm_after_end(issue)?;
            ended
        }
        End::Review(reviewed) => {
            let line = open.and_then(|open| runner.review_line(&open, &reviewed));
            let unreported = !matches!(reviewed.spent, Some(Spent::Claude { .. } | Spent::Local));
            let ended = runner.on(Some(issue)).end_review(reviewed);
            runner.flights.flying.remove(&issue);
            if let (Some(line), Ok(_)) = (line, &ended) {
                runner.append_call(line, unreported);
            }
            ended
        }
        End::Panicked(panic) => {
            runner.flights.flying.remove(&issue);
            if let Some(open) = &open {
                runner.panicked_line(open);
            }
            drop(runner);
            resume_unwind(panic)
        }
    }
}

mod calls;
mod carry;
pub(super) mod label;
pub(super) mod panel;
mod proving;

pub(in crate::runner) use proving::Verdict;
pub(super) mod retro;
pub(super) mod security;
#[cfg(test)]
mod step;
#[cfg(test)]
pub(crate) mod tests;
pub(super) mod triage;
mod waiting;

pub use carry::CarriedTurn;
#[cfg(test)]
pub use step::step;
pub(super) use waiting::Waiting;
