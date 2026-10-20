//! The worker's turns
//!
//! A turn begins (prepare the worktree and profile, mark the turn running,
//! save), its call runs in flight ([`super::flight`]), and a later pass
//! ends it (record the call, save). A turn still marked running when the
//! runner starts was cut short, and its session is resumed. A session cut
//! short before it wrote anything starts over. A turn that ends on a
//! question block parks the worker on a ruling.

use std::num::NonZeroU32;
use std::path::Path;
use std::time::Duration;

use super::Runner;
use super::adopt;
use super::browser;
use super::claim::main_fetch_key;
use super::claude_files::Unchecked;
use super::instructions;
use super::pace::Pace;
use super::pm::WORKER_COMPACT_PAST;
use super::pull_request_file;
use super::question::asked;
use super::report::{Begin, StepReport};
use super::rework;
use super::ruling::park;
use crate::guard::Pause;
use crate::pacer::Scope;
use crate::ports::{
    AgentCall, AgentError, AgentReply, COMPACT, Cost, InstallationToken, Issue, Reach, Role,
    Session, ThreadComment, Timestamp, Tools,
};
use crate::profile::{self, WorkerProfile};
use crate::settings::{AgentHarness, Effort, Limit};
use crate::skills::{Step, split_command};
use crate::state::{Resume, RulingKind, StateError};
use crate::ticket::TicketId;
use crate::work_item::{Phase, Review, ReviewStage, Seat, Turn, WorkItem};
use crate::worker_folder;
use crate::worktree::{self, Start};
use handover::{Handover, Refused};
use held_up::Reason;
use unfinished::{awaits_a_push, timed_out, uncommitted_prompt};
pub(super) use unfinished::{failed, named_files};

mod budget;
mod dep_reads;
mod ended;
mod handover;
pub(super) mod held_up;
mod issue_comments;
pub(super) mod paused;
mod unfinished;
pub(super) mod unreachable;

/// What a work item's worker runs on this turn
pub(super) struct WorkerAgent {
    /// Its harness
    pub(super) harness: AgentHarness,
    /// What holds its turns back
    pub(super) limit: Limit,
    /// The model, as the harness takes it
    pub(super) model: String,
    /// How hard the model thinks
    pub(super) effort: Effort,
    /// The tools its agent file names, in place of a worker's own
    pub(super) tools: Option<String>,
    /// Its agent file's body, added to kelpie's own instructions
    pub(super) prompt: Option<String>,
}

/// The prompt for a turn resumed after the runner restarted
pub(super) const CONTINUE: &str = "Kelpie restarted while your last turn was running. \
                                   Carry on with the work item from where you left off.";

/// What every fix turn's prompt asks, so the pull request's body stays true
/// to the head the turn pushes
///
/// It sits where the prompt asks for the push. A prompt may add further
/// instructions after it, such as a review bot's thread lines.
///
/// It says nothing of how: the worker's instructions do ([`profile`]), for
/// a worker that edits the pull request itself and for one that rewrites
/// the file kelpie sets the body from.
pub(super) const KEEP_BODY_TRUE: &str = "Update the pull request's body so it matches the head \
    you push: what changed, the files touched, the test evidence, and the decisions you made.";

/// How a worker's prompt names its ticket
///
/// A GitHub ticket is an issue, which the worker can read with `gh`. A ticket
/// from another source is only a ticket.
#[must_use]
pub(super) fn ticket_name(ticket: TicketId) -> String {
    match ticket.github() {
        Some(_) => format!("issue #{ticket}"),
        None => format!("ticket {ticket}"),
    }
}

// The title and body, then the comments the maintainer wrote on the issue
// before this turn began, if any.
fn first_prompt(ticket: TicketId, issue: &Issue, comments: &[ThreadComment]) -> String {
    format!(
        "Your work item is {}: {}\n\n{}\n{}",
        ticket_name(ticket),
        issue.title,
        issue.body.trim_end(),
        issue_comments::block(ticket, comments)
    )
}

/// What a step can work on
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Slot {
    /// The open work item for this ticket
    Item(TicketId),
    /// The board, while a slot is free under `concurrency.active_items` and fewer than
    /// `concurrency.pending_rulings` items are parked on rulings other than a follow-up.
    /// 0 counts as 1: the board opens nothing while any such ruling waits.
    Board,
    /// The contributions' calls, while a slot is free and the board is closed
    /// (by parked rulings, or by finishing), so they still start
    Contributions,
}

impl Runner {
    // Steps each open work item with no call in flight, and the board while
    // a slot is free, from the one after the last to act, until one acts.
    // `start_over` is the item whose session died unborn, which begins the
    // same turn again.
    pub(super) fn begin_turn(&mut self, start_over: Option<TicketId>) -> Result<Begin, StateError> {
        self.snapshot.begin_pass();
        self.looks.pass_began(self.forge_ledger.writes());
        // What follows asks the forge with the runner's lock held.
        self.post();
        // Read `main`'s workflows before any step is chosen, so no forge read
        // falls between a step's drain check and the call it starts.
        self.read_main_ci();
        let begin = self.step_rotation(start_over);
        self.snapshot.end_pass();
        begin
    }

    fn step_rotation(&mut self, start_over: Option<TicketId>) -> Result<Begin, StateError> {
        if let Some(ticket) = start_over {
            self.focus = Some(ticket);
            let begin = if self.current().is_some_and(|item| item.attached.is_some()) {
                self.held_up.because(Reason::Attached);
                Begin::Idle
            } else {
                self.begin_item(true)?
            };
            self.note_held_up(ticket, &begin);
            return Ok(begin);
        }
        self.recheck_dropped()?;
        self.follow_contributions()?;
        self.follow_dependencies()?;
        self.follow_triage()?;
        let open: Vec<TicketId> = self.state.work_items.iter().map(|i| i.issue).collect();
        self.unreachable.keep_only(&open);
        self.held_up.keep_only(&open);
        self.seat_waiting()?;
        let mut waiting = None;
        let now = self.ports.clock.now();
        let rotation = self.rotation();
        self.note_left_out(&rotation, now);
        self.looks.keep_only(&rotation);
        for slot in rotation {
            if !self.looks.due(slot, now) {
                continue;
            }
            // The step ahead asks the forge with the runner's lock held, so
            // post what a trigger reads before it waits.
            self.post();
            let begin = match slot {
                Slot::Item(ticket) => {
                    self.focus = Some(ticket);
                    let begin = self.begin_item(false)?;
                    self.note_held_up(ticket, &begin);
                    begin
                }
                Slot::Board => {
                    self.focus = None;
                    self.looks.reading_board(now);
                    self.dispatch()?
                }
                Slot::Contributions => {
                    self.focus = None;
                    self.contribution_due()?.unwrap_or(Begin::Idle)
                }
            };
            let held = self.forge_hold.until(now);
            self.looks.stepped(slot, &begin, now, held);
            match begin {
                Begin::Idle => {}
                Begin::Report(report) if report.waits() => {
                    waiting.get_or_insert(report);
                }
                begin => {
                    self.last_acted = Some(slot);
                    return Ok(begin);
                }
            }
        }
        Ok(waiting.map_or(Begin::Idle, Begin::Report))
    }

    // Remembers why `ticket`'s step began `begin`, and logs a turn held up long
    fn note_held_up(&mut self, ticket: TicketId, begin: &Begin) {
        let now = self.ports.clock.now();
        if let Some(line) = self.held_up.stepped(ticket, begin, now) {
            self.notes.push(line);
        }
    }

    // Says what holds back each open work item the rotation leaves out, whose
    // turn is due, and forgets the wait of the rest: the rotation never steps
    // them, so no step would
    fn note_left_out(&mut self, rotation: &[Slot], now: Timestamp) {
        let left_out: Vec<(TicketId, Option<Reason>)> = (self.state.work_items.iter())
            .filter_map(|i| {
                let ticket = i.issue;
                if rotation.contains(&Slot::Item(ticket)) {
                    return None;
                }
                let due = matches!(i.turn, Turn::Due | Turn::Next { .. });
                let reason = if !due || self.flights.flying(ticket) {
                    None
                } else if i.attached.is_some() {
                    Some(Reason::Attached)
                } else {
                    (i.seat == Seat::Waiting).then_some(Reason::Unseated)
                };
                Some((ticket, reason))
            })
            .collect();
        for (ticket, reason) in left_out {
            if let Some(reason) = reason {
                self.held_up.because(reason);
            }
            if let Some(line) = self.held_up.stepped(ticket, &Begin::Idle, now) {
                self.notes.push(line);
            }
        }
    }

    // The open work items with no call in flight, none attached and none
    // waiting for a slot, oldest first, then the board while it may open
    // one, then the contributions' calls while the board is closed and a slot
    // is free, from the one after the last to act, so one that keeps acting
    // cannot starve the rest
    pub(super) fn rotation(&self) -> Vec<Slot> {
        let items = (self.state.work_items.iter()).filter_map(|i| {
            let ticket = i.issue;
            (!self.flights.flying(ticket) && i.attached.is_none() && i.seat != Seat::Waiting)
                .then_some(Slot::Item(ticket))
        });
        let board = self.board_open().then_some(Slot::Board);
        let contributions = (board.is_none() && self.slot_free() && self.contribution_unread())
            .then_some(Slot::Contributions);
        let mut slots: Vec<Slot> = items.chain(board).chain(contributions).collect();
        if let Some(at) = slots.iter().position(|&s| Some(s) == self.last_acted) {
            slots.rotate_left(at + 1);
        }
        slots
    }

    // Idle for `reason`, which `status` and the board name
    fn idle(&mut self, reason: Reason) -> Begin {
        self.held_up.because(reason);
        Begin::Idle
    }

    fn begin_item(&mut self, start_over: bool) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        if self.unseated() {
            return Ok(self.idle(Reason::Unseated));
        }
        let owes_replies =
            item.talked.head.is_some() && matches!(item.phase, Phase::Review(_) | Phase::Ci { .. });
        if owes_replies && let Some(begin) = self.answer_talk()? {
            return Ok(begin);
        }
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        match &item.phase {
            Phase::Implement => {}
            // A fix turn, or a turn to push before a round, that ended goes
            // back to its round, to check it pushed.
            Phase::Review(review)
                if matches!(
                    review.stage,
                    ReviewStage::Fixing { .. } | ReviewStage::Pushing
                ) && !matches!(item.turn, Turn::Ended { .. }) => {}
            Phase::Review(_) => {
                if let Some(parked) = self.fence_gate()? {
                    return Ok(parked);
                }
                return self.review_step();
            }
            Phase::Ci { .. } => return self.check_ci(),
            Phase::Ruling { id } => return self.late_while_parked(*id),
            Phase::Merge { .. } => return self.merge(),
            Phase::Done { merged, .. } => return self.finish(*merged),
        }
        // A turn whose model host could not be reached is tried again as it
        // was, once its pause has passed.
        if !start_over && let Some(retry) = self.retry_for_host(item.issue) {
            return Ok(retry);
        }
        // Only a turn that has not begun waits on the pacer: one already
        // running carries on, since a turn is never interrupted, and start_over
        // resumes the same not-yet-begun turn after a session died unborn.
        let due = matches!(item.turn, Turn::Due | Turn::Next { .. });
        if let Some(begin) = self.pushed_by_someone_else()? {
            if matches!(begin, Begin::Idle) {
                self.held_up.because(Reason::PushedByAnother);
            }
            return Ok(begin);
        }
        // A rework can start on a branch that already changes them.
        if due && let Some(parked) = self.claude_files_changed(Unchecked::CarryOn)? {
            return Ok(parked);
        }
        if due && let Some(parked) = self.park_over_budget()? {
            return Ok(parked);
        }
        if due && let Some(why) = self.builds_hold() {
            return Ok(self.idle(why));
        }
        if due {
            let pace = self.pace_worker(Scope::Turn)?;
            if let Pace::StillHeld(hold) | Pace::Held(hold) = &pace {
                self.held_up.because(Reason::Paced(hold.reason.clone()));
            }
            if let Some(held) = pace.holds() {
                return Ok(held);
            }
        }
        let item = self.current().expect("checked above");
        let id = item.session.clone();
        let now = self.ports.clock.now();
        // A turn already running when the runner starts keeps the start it
        // was saved with, so a restart can never buy it a fresh ceiling: the
        // call it resumes gets only however much of the ceiling is left.
        let (session, prompt, since) = match &item.turn {
            Turn::Due => (Session::New(id), None, now),
            Turn::Running { since, .. } if start_over => (Session::New(id), None, *since),
            Turn::Running { since, .. } => (Session::Resume(id), Some(CONTINUE.to_owned()), *since),
            // An adopted work item's session begins with whatever the gate sends it.
            Turn::Next { prompt } if adopt::unborn(item) => {
                let (command, then) = split_command(prompt);
                let first = adopt::first_prompt(item, Some(then));
                let first = match command {
                    Some(command) => format!("{command} {first}"),
                    None => first,
                };
                (Session::New(id), Some(first), now)
            }
            // No session to resume: the retry of a turn whose call failed
            // before its session existed starts it over, as a killed one does.
            Turn::Next { .. } if start_over => (Session::New(id), None, now),
            Turn::Next { prompt } => (Session::Resume(id), Some(prompt.clone()), now),
            // A first turn that ended with no pull request and no question
            // stopped short, maybe on a tool that failed.
            Turn::Ended { .. } if item.pull_request.is_none() || awaits_a_push(item) => {
                return self.stopped_short();
            }
            Turn::Ended { .. } | Turn::Failed { .. } => return Ok(Begin::Idle),
        };
        // A turn the runner sends into a session grown past the mark is
        // sent behind a `/compact`, which leaves the turn due as it was.
        let compacting = matches!(item.turn, Turn::Next { .. })
            && matches!(session, Session::Resume(_))
            && item.context.is_some_and(|c| c > WORKER_COMPACT_PAST);
        // What a yes on this turn's failure sends again: the prompt a resumed
        // turn was sent, which a restart of the turn leaves as it was.
        let kept = match &item.turn {
            Turn::Next { prompt } if !start_over => Some(prompt.clone()),
            Turn::Running { prompt, .. } if !start_over => prompt.clone(),
            _ => None,
        };
        let prompt = if compacting {
            Some(COMPACT.to_owned())
        } else {
            prompt
        };
        let ceiling = self.turn_ceiling();
        let elapsed = Duration::from_secs(now.0.saturating_sub(since.0));
        let remaining = ceiling.saturating_sub(elapsed);
        if remaining.is_zero() {
            // The ceiling passed while kelpie was down, before a new call
            // could even be tried: park it as a call that hit
            // `AgentError::TimedOut` would, with no call spent.
            return self.park_ceiling_passed(now);
        }
        if self.draining() {
            return Ok(self.idle(Reason::Draining));
        }
        let issue = item.issue;
        let ticket = item.issue;
        // A later turn restarted with a new session, as one that found no
        // session to resume is, has a worker call on record and is not first.
        let first = matches!(session, Session::New(_))
            && prompt.is_none()
            && !(item.rework || item.adopted)
            && !item.calls.iter().any(|c| c.role == Role::Worker);
        // A token GitHub would not give a moment ago is not asked for at every pass.
        if self.token_wait.holds(now) {
            return Ok(self.idle(Reason::Token));
        }
        self.token_wait.begins();
        let prepared = self.prepare(item, session, prompt);
        let mut next = self.state.clone();
        let mut resumed = false;
        let mut begin = match prepared {
            // A covered worker never runs as the maintainer, so a token that
            // passes waits here and is tried again, not parked on a ruling.
            Err(reason) if self.token_wait.is_busy() => {
                if self.token_wait.first_told() {
                    self.notes
                        .push(format!("issue #{issue} waits to start its turn: {reason}"));
                }
                return Ok(self.idle(Reason::Token));
            }
            Ok(call) => {
                // `prepare` mints the worker's GitHub token and prepares its
                // worktree with the runner's lock held, and a `drain` that
                // lands in that time sets its flag there without waiting for
                // it. Checked again here, so the turn stays due rather than
                // starting past the drain.
                if self.draining() {
                    self.token_wait.ends();
                    // No flight will take the bridge this turn's call opened.
                    drop(self.opened.take(issue));
                    return Ok(self.idle(Reason::Draining));
                }
                self.token_wait.ends();
                resumed = !compacting && paused::resumes_pause(item);
                self.builds_turn_begins();
                let item = self
                    .current_in(&mut next)
                    .expect("the work item checked above");
                if !compacting {
                    item.turn = Turn::Running {
                        since,
                        prompt: kept,
                    };
                }
                let deadline = Timestamp(since.0.saturating_add(ceiling.as_secs()));
                Begin::Call {
                    call,
                    deadline,
                    first,
                    compact: compacting,
                }
            }
            Err(reason) => {
                let names = self.names();
                let restore = self.restorable(issue);
                Begin::Report(failed(names, &mut next, issue, now, reason, restore))
            }
        };
        if let Err(e) = self.save(next) {
            // No flight will take the bridge this turn's call opened.
            drop(self.opened.take(issue));
            return Err(e);
        }
        if resumed {
            self.note_resumed(ticket);
        }
        if let Begin::Report(report) = &mut begin {
            self.fill_comment_failed(report);
        }
        Ok(begin)
    }

    // Everything the worker needs on disk before it starts: its worktree, its
    // build folder, its settings file and kelpie's instructions. A turn with
    // no prompt of its own is the first, and takes the issue, the review or
    // what it adopted.
    pub(super) fn prepare(
        &self,
        item: &WorkItem,
        session: Session,
        prompt: Option<String>,
    ) -> Result<AgentCall, String> {
        // A session with no prompt (attach) or only a compaction runs no browser.
        let browses = prompt
            .as_deref()
            .is_none_or(|p| !p.is_empty() && p != COMPACT);
        let call = self.worker_call(item, session, prompt, browses)?;
        self.prepared(call)
    }

    // The worker's call for `item`, with its worktree, build folder, settings
    // path and instructions made, before its harness has written anything.
    // `browses` is whether its instructions may name a browser, which only a
    // turn that goes on to open one has.
    pub(super) fn worker_call(
        &self,
        item: &WorkItem,
        session: Session,
        prompt: Option<String>,
        browses: bool,
    ) -> Result<AgentCall, String> {
        let start = start_of(item);
        let WorkerAgent {
            harness,
            limit: _,
            model,
            effort,
            tools: agent_tools,
            prompt: agent_prompt,
        } = self.worker_agent(item)?;
        // Where the App covers the repo, the worker holds its token and never
        // the maintainer's login; a token it cannot have now fails the call
        // rather than run it as the maintainer.
        let token = (self.ports.voice.worker_token(&self.remote)).map_err(|e| {
            self.token_error_passes(&e);
            format!("cannot get the worker a GitHub token: {e}")
        })?;
        let reach = self.worker_reach(item, start, token.as_ref())?;
        // Named for the work item: the open items' calls run at once, and
        // each harness keeps its own files beside its settings file.
        let folder = &self.paths.worker;
        let settings = folder.join(format!("settings-{}.json", item.issue));
        let instructions = folder.join(format!("instructions-{}.md", item.issue));
        let handed_over = token
            .is_some()
            .then(|| pull_request_file::path(&item.build));
        let hosts = (self.browser_hosts(&harness, &session, Some(item.issue))).filter(|_| browses);
        let dev = self.dev_server(item, &reach, hosts);
        // The owner's template reaches the worker as a file in its build folder,
        // which it reads when it writes its body. Written on every call, so a
        // template kelpie reread is the one the next read finds. The worktree's
        // own template outranks it, and then no file is written.
        let owners = self
            .owner_template
            .as_ref()
            .filter(|_| instructions::template(&item.worktree).is_none());
        let template_file = match owners {
            Some(owners) => {
                let path = item.build.join(instructions::TEMPLATE_FILE);
                write(&item.build, &path, &owners.text())?;
                Some(path)
            }
            None => None,
        };
        let text = instructions::compose(
            instructions::Extra {
                agent: agent_prompt.as_deref(),
                project: self.extra_instructions.as_deref(),
                // Its token opens no pull request, so kelpie opens it.
                pull_request_file: handed_over.as_deref(),
                e2e: self.settings.e2e.as_ref(),
                owner_template: owners.zip(template_file.as_deref()),
                browser: hosts,
                dev_server: dev.as_ref(),
                local_ticket: item.issue.github().is_none(),
                base: Some(self.settings.git.base()),
            },
            &item.worktree,
            &self.skills,
            &self.kelpie,
        );
        write(folder, &instructions, &text)?;
        // A pause set for an earlier call, or one that ended it, is not this call's.
        Pause::in_build(&item.build).clear();
        let prompt = match prompt {
            Some(prompt) => prompt,
            None if item.rework => rework::first_prompt(item),
            None if item.adopted => adopt::first_prompt(item, None),
            None => {
                let ticket = item.issue;
                let issue = self
                    .ports
                    .tickets
                    .issue(&self.remote, ticket)
                    .map_err(|e| format!("cannot read {}: {e}", ticket.shown()))?;
                let comments = self.maintainer_comments(ticket)?;
                self.skills
                    .invoke(Step::Implement, &first_prompt(ticket, &issue, &comments))
            }
        };
        // A `/compact` has nothing to bound, and would hand itself back after.
        let compact_window =
            NonZeroU32::new(self.settings.worker.compact_window).filter(|_| prompt != COMPACT);
        Ok(AgentCall {
            issue: Some(item.issue),
            settings,
            instructions: Some(instructions),
            plugin_dirs: self.skills.plugin_dirs().to_vec(),
            agent_tools,
            reach,
            compact_window,
            ..AgentCall::new(
                Role::Worker,
                Tools::Work,
                harness,
                model,
                effort,
                session,
                item.worktree.clone(),
                prompt,
            )
        })
    }

    /// What a session working in `item`'s worktree reaches: the worker's own
    /// fence, with its worktree and build folder made ready
    ///
    /// # Errors
    ///
    /// Why the worktree cannot be made ready, or why agents' own files in it
    /// are refused.
    pub(super) fn worker_reach(
        &self,
        item: &WorkItem,
        start: Start,
        token: Option<&InstallationToken>,
    ) -> Result<Reach, String> {
        let dirs = worktree::prepare(
            &self.settings.git.checkout,
            &item.worktree,
            &item.branch,
            start,
            self.settings.git.base(),
            &item.remote,
            &item.build,
        )
        .map_err(|e| e.to_string())?;
        // The worker merges `origin/<base>` and runs no fetch of its own, so a
        // fetch that failed leaves it merging a base kelpie last fetched.
        let key = main_fetch_key(item.issue);
        match &dirs.main_fetch_failed {
            Some(why) => self.notes.standing(
                &key,
                format!(
                    "issue #{}'s worker runs with `origin/{}` where kelpie last fetched it: \
                     {why}",
                    item.issue,
                    self.settings.git.base()
                ),
            ),
            None => self.notes.ends(&key),
        }
        if let Some(reason) = self.claude_files_refusal() {
            return Err(reason);
        }
        // Its login is written for this call, and a call whose login cannot be
        // written does not start: `gh` would find the maintainer's instead.
        if let Some(token) = token {
            profile::write_login(&item.build, token)
                .map_err(|e| format!("cannot write the worker's GitHub login: {e}"))?;
        }
        let profile = WorkerProfile {
            worktree: &item.worktree,
            build: &item.build,
            git_common_dir: &dirs.git_common_dir,
            git_dir: &dirs.git_dir,
            branch: &item.branch,
            base: self.settings.git.base(),
            kelpie: &self.kelpie,
            guard_hooks: &self.settings.worker.guard_hooks,
            kelpie_home: &self.paths.kelpie_home,
            repo: &self.settings.git.checkout,
            allowed_domains: &self.settings.worker.allowed_domains,
            build_env: &self.settings.worker.build_env,
            shep_home: &self.paths.shep_home,
            door: &self.paths.door,
            token,
        };
        // The guard in the fence runs the project's `commit`, `push` and
        // `publish` hooks, so every harness reaches them with its own checks.
        let mut reach = profile.reach();
        if let Some(fence) = reach.fence.as_mut() {
            fence.guard.hooks = self.hooks.clone().in_folder(&item.worktree);
        }
        Ok(reach)
    }

    /// The agent `item`'s worker runs on: the one it opened on, as that
    /// agent's file now stands
    ///
    /// # Errors
    ///
    /// Why not, when kelpie no longer has that agent, or the worker's fence
    /// would not hold on it.
    pub(super) fn worker_agent(&self, item: &WorkItem) -> Result<WorkerAgent, String> {
        let name = &item.agent;
        let Some(agent) = self.book.get(name) else {
            return Err(format!(
                "issue #{} runs on agent {name}, which has no agent file now: write \
                 `agents/{name}.md` in kelpie's home again, or drop and add the issue",
                item.issue
            ));
        };
        let Some((model, limit)) = agent
            .runs
            .session()
            .filter(|_| agent.role == crate::agents::Role::Implementer)
        else {
            return Err(format!(
                "issue #{} runs on agent {name}, whose file is a reviewer's now: give \
                 `agents/{name}.md` in kelpie's home `role: implementer` again, or drop and \
                 add the issue",
                item.issue
            ));
        };
        self.settings
            .under_worker_fence(name, model)
            .map_err(|e| e.to_string())?;
        Ok(WorkerAgent {
            harness: model.harness.clone(),
            limit: limit.clone(),
            model: model.model.as_str().to_owned(),
            effort: model.effort,
            tools: model.tools.as_ref().map(|t| t.as_str().to_owned()),
            prompt: agent.prompt.clone(),
        })
    }

    /// `call`, once its harness has what it needs on disk
    ///
    /// Done before the call is marked running, so a failure keeps the turn
    /// it would have run for a retry.
    pub(super) fn prepared(&self, mut call: AgentCall) -> Result<AgentCall, String> {
        // The plugins the repo turns on reach every call the runner makes in it,
        // unless the project's `repo_setup` keeps them from this call's role.
        if self.settings.repo_setup.reaches(call.role) {
            // A call in a worktree that matches `main` loads the repo's own
            // `.claude` files from it already, and must not get them twice.
            let own_loaded = call.harness.reads_claude_files()
                && crate::fence::matches_main(
                    &self.settings.git.checkout,
                    &call.cwd,
                    self.settings.git.base(),
                );
            for plugin in self.repo_plugins.dirs(own_loaded) {
                if !call.plugin_dirs.iter().any(|dir| dir == plugin) {
                    call.plugin_dirs.push(plugin.to_owned());
                }
            }
            // Their MCP servers go in the call's own config, since Claude Code's
            // `--strict-mcp-config` drops every other. A call that only reads (a vote, a retro)
            // is given none that could write.
            if call.harness.loads_mcp() && call.tools != Tools::Retro {
                // A server the call already names is kelpie's own (a researcher's web), which
                // a repo server of the same name does not replace. goose lowercases a name into
                // its key, so a name that differs only in case is no other server either.
                for (name, entry) in self.repo_plugins.mcp_servers() {
                    let held =
                        (call.reach.mcp_servers.keys()).any(|k| k.eq_ignore_ascii_case(&name));
                    if !held {
                        call.reach.mcp_servers.insert(name, entry);
                    }
                }
                self.without_servers_the_harness_cannot_load(&mut call);
            }
        }
        self.with_dep_sources(&mut call);
        self.with_code_graph(&mut call);
        self.with_browser(&mut call)?;
        if let Err(e) = self.ports.agents.prepare(&call) {
            // No flight will take the bridge `with_browser` opened.
            if let Some(ticket) = browser::ticket_of(&call) {
                drop(self.opened.take(ticket));
            }
            return Err(e.to_string());
        }
        Ok(call)
    }

    // The open pull request from `branch`. A forge that cannot be asked
    // leaves it unrecorded, and status shows none.
    pub(super) fn pull_request_from(&self, branch: &str) -> Option<u64> {
        let open = self.ports.forge.open_pull_requests(&self.remote).ok()?;
        open.into_iter()
            .find(|pr| pr.head == branch)
            .map(|pr| pr.number)
    }
}

// Where a worktree made for `item` starts: a rework or adopted branch is the
// one `origin` holds.
pub(super) fn start_of(item: &WorkItem) -> Start {
    if item.rework || item.adopted {
        Start::Pushed
    } else {
        Start::Main
    }
}

pub(super) fn write(folder: &Path, file: &Path, text: &str) -> Result<(), String> {
    let failed = |why: &dyn std::fmt::Display| format!("cannot write {}: {why}", file.display());
    let rel = file
        .strip_prefix(folder)
        .map_err(|_| failed(&"it is not in the folder"))?;
    worker_folder::write(folder, rel, text).map_err(|e| failed(&e))
}

#[cfg(test)]
mod tests;
