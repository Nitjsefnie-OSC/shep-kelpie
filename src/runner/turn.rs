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
use super::claude_files::Unchecked;
use super::instructions;
use super::pm::{COMPACT, WORKER_COMPACT_PAST};
use super::pull_request_file;
use super::question::asked;
use super::report::{Begin, StepReport};
use super::rework;
use super::ruling::park;
use crate::github::InstallationToken;
use crate::pacer::Scope;
use crate::ports::{
    AgentCall, AgentError, AgentReply, Cost, Issue, Reach, Role, Session, Timestamp, Tools,
};
use crate::profile::{self, WorkerProfile};
use crate::settings::{AgentHarness, Effort, Limit};
use crate::skills::{Step, split_command};
use crate::state::{Resume, RulingKind, StateError};
use crate::work_item::{Phase, Review, ReviewStage, Seat, Turn, WorkItem};
use crate::worker_folder;
use crate::worktree::{self, Start};
use handover::{Handover, Refused};
use unfinished::{awaits_a_push, timed_out, uncommitted_prompt};
pub(super) use unfinished::{failed, named_files};

mod handover;
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

fn first_prompt(number: u64, issue: &Issue) -> String {
    format!(
        "Your work item is issue #{number}: {}\n\n{}\n",
        issue.title,
        issue.body.trim_end()
    )
}

/// What a step can work on
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Slot {
    /// The open work item for this issue
    Item(u64),
    /// The board, while a slot is free under `concurrency.active_items` and fewer than
    /// `concurrency.pending_rulings` items are parked on rulings other than a follow-up.
    /// 0 counts as 1: the board opens nothing while any such ruling waits.
    Board,
}

impl Runner {
    // Steps each open work item with no call in flight, and the board while
    // a slot is free, from the one after the last to act, until one acts.
    // `start_over` is the item whose session died unborn, which begins the
    // same turn again.
    pub(super) fn begin_turn(&mut self, start_over: Option<u64>) -> Result<Begin, StateError> {
        if let Some(issue) = start_over {
            self.focus = Some(issue);
            if self.current().is_some_and(|item| item.attached.is_some()) {
                return Ok(Begin::Idle);
            }
            return self.begin_item(true);
        }
        self.recheck_dropped()?;
        let open: Vec<u64> = self.state.work_items.iter().map(|i| i.issue).collect();
        self.unreachable.keep_only(&open);
        self.seat_waiting()?;
        let mut waiting = None;
        let now = self.ports.clock.now();
        let rotation = self.rotation();
        self.looks.keep_only(&rotation);
        for slot in rotation {
            if !self.looks.due(slot, now) {
                continue;
            }
            let begin = match slot {
                Slot::Item(issue) => {
                    self.focus = Some(issue);
                    self.begin_item(false)?
                }
                Slot::Board => {
                    self.focus = None;
                    self.looks.reading_board(now);
                    self.dispatch()?
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

    // The open work items with no call in flight, none attached and none
    // waiting for a slot, oldest first, then the board while it may open
    // one, from the one after the last to act, so one that keeps acting
    // cannot starve the rest
    pub(super) fn rotation(&self) -> Vec<Slot> {
        let items = (self.state.work_items.iter())
            .filter(|i| {
                !self.flights.flying(i.issue) && i.attached.is_none() && i.seat != Seat::Waiting
            })
            .map(|i| Slot::Item(i.issue));
        let board = self.board_open().then_some(Slot::Board);
        let mut slots: Vec<Slot> = items.chain(board).collect();
        if let Some(at) = slots.iter().position(|&s| Some(s) == self.last_acted) {
            slots.rotate_left(at + 1);
        }
        slots
    }

    fn begin_item(&mut self, start_over: bool) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        if self.unseated() {
            return Ok(Begin::Idle);
        }
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
            return Ok(begin);
        }
        // A rework can start on a branch that already changes them.
        if due && let Some(parked) = self.claude_files_changed(Unchecked::CarryOn)? {
            return Ok(parked);
        }
        if due && let Some(held) = self.pace_worker(Scope::Turn)?.holds() {
            return Ok(held);
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
        if self.draining {
            return Ok(Begin::Idle);
        }
        let issue = item.issue;
        // A later turn restarted with a new session, as one that found no
        // session to resume is, has a worker call on record and is not first.
        let first = matches!(session, Session::New(_))
            && prompt.is_none()
            && !(item.rework || item.adopted)
            && !item.calls.iter().any(|c| c.role == Role::Worker);
        // A token GitHub would not give a moment ago is not asked for at every pass.
        if self.token_wait.holds(now) {
            return Ok(Begin::Idle);
        }
        self.token_wait.begins();
        let prepared = self.prepare(item, session, prompt);
        let mut next = self.state.clone();
        let mut begin = match prepared {
            // A covered worker never runs as the maintainer, so a token that
            // passes waits here and is tried again, not parked on a ruling.
            Err(reason) if self.token_wait.is_busy() => {
                if self.token_wait.first_told() {
                    self.notes
                        .push(format!("issue #{issue} waits to start its turn: {reason}"));
                }
                return Ok(Begin::Idle);
            }
            Ok(call) => {
                self.token_wait.ends();
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
                Begin::Report(failed(names, &mut next, issue, now, reason))
            }
        };
        self.save(next)?;
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
        let call = self.worker_call(item, session, prompt)?;
        self.prepared(call)
    }

    // The worker's call for `item`, with its worktree, build folder, settings
    // path and instructions made, before its harness has written anything.
    pub(super) fn worker_call(
        &self,
        item: &WorkItem,
        session: Session,
        prompt: Option<String>,
    ) -> Result<AgentCall, String> {
        let start = if item.rework || item.adopted {
            Start::Pushed
        } else {
            Start::Main
        };
        let WorkerAgent {
            harness,
            limit,
            model,
            effort,
            tools: agent_tools,
            prompt: agent_prompt,
        } = self.worker_agent(item)?;
        // Where the App covers the repo, the worker holds its token and never
        // the maintainer's login; a token it cannot have now fails the call
        // rather than run it as the maintainer.
        let token = (self.ports.voice.worker_token(&self.remote)).map_err(|e| {
            if matches!(e, crate::ports::ForgeError::AppBusy(_)) {
                self.token_wait.busy_at(self.ports.clock.now());
            }
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
        let text = instructions::compose(
            instructions::Extra {
                agent: agent_prompt.as_deref(),
                project: self.extra_instructions.as_deref(),
                // Its token opens no pull request, so kelpie opens it.
                pull_request_file: handed_over.as_deref(),
            },
            &item.worktree,
            &self.skills,
            &self.kelpie,
        );
        write(folder, &instructions, &text)?;
        let prompt = match prompt {
            Some(prompt) => prompt,
            None if item.rework => rework::first_prompt(item),
            None if item.adopted => adopt::first_prompt(item, None),
            None => {
                let issue = self
                    .ports
                    .forge
                    .issue(&self.remote, item.issue)
                    .map_err(|e| format!("cannot read issue #{}: {e}", item.issue))?;
                self.skills
                    .invoke(Step::Implement, &first_prompt(item.issue, &issue))
            }
        };
        // A `/compact` has nothing to bound, and would hand itself back after.
        let compact_window =
            NonZeroU32::new(self.settings.worker.compact_window).filter(|_| prompt != COMPACT);
        Ok(AgentCall {
            role: Role::Worker,
            harness,
            issue: item.issue,
            model,
            effort,
            session,
            cwd: item.worktree.clone(),
            settings,
            instructions: Some(instructions),
            prompt,
            plugin_dirs: self.skills.plugin_dirs().to_vec(),
            tools: Tools::Work,
            agent_tools,
            reach,
            lease: limit.lease().cloned(),
            compact_window,
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
            &item.build,
        )
        .map_err(|e| e.to_string())?;
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
    pub(super) fn prepared(&self, call: AgentCall) -> Result<AgentCall, String> {
        self.ports
            .agents
            .prepare(&call)
            .map_err(|e| e.to_string())?;
        Ok(call)
    }

    pub(super) fn end_turn(
        &mut self,
        result: Result<AgentReply, AgentError>,
    ) -> Result<Option<StepReport>, StateError> {
        // A turn stopped with the runner stays running, to resume on restart.
        // Its time so far is saved to the worker, which still runs it.
        if matches!(result, Err(AgentError::Stopped)) {
            return self.save_time().map(|()| None);
        }
        let now = self.ports.clock.now();
        // Whatever the turn left on `origin` is the worker's own. A head
        // that cannot be read keeps the last one, which errs toward parking.
        let pushed = self.current().and_then(|_| self.own_push());
        // Whether the head on `origin` is the worktree's own commit, which a
        // push by anyone else is not.
        let own_commit = pushed.is_some() && self.worktree_head() == pushed;
        let mut next = self.state.clone();
        let Some(item) = self.current_in(&mut next) else {
            return Ok(None);
        };
        let issue = item.issue;
        // A rework or adoption turn that moved nothing on `origin` pushed no fix.
        let before = item.known.head.clone();
        let no_push = pushed.as_ref().is_none_or(|p| Some(p) == before.as_ref());
        let pushed_nothing = before.is_some() && no_push;
        if pushed.is_some() {
            item.known.head = pushed;
        }
        let report = match result {
            Ok(reply) => {
                let question = asked(&reply.text);
                // Only true the very first time: the pull request is
                // discovered once, and every later turn that reaches here
                // (a CI-failure fix, most often) already knows it.
                let discovering = item.pull_request.is_none();
                // Only a worker on a repo the App covers hands its pull request
                // over: any other opens and edits its own, and a file it left
                // is not kelpie's to act on.
                let handed_over = self.app_covers();
                if discovering {
                    // Only this turn's refusal is told: an earlier one no longer
                    // says why there is no pull request.
                    item.pull_request_refused = None;
                    item.pull_request = self.pull_request_from(&item.branch);
                    // A worker whose token opens no pull request wrote its text
                    // for kelpie to open it from, once the branch is pushed.
                    if handed_over && item.pull_request.is_none() && item.known.head.is_some() {
                        match self.open_handed_over(&item.build, &item.branch) {
                            Ok(opened) => item.pull_request = opened,
                            // The worker can fix its own file, so it is told why.
                            Err(refusal) => {
                                item.pull_request_refused = Some(pull_request_file::told(&refusal));
                                self.started.push(StepReport::PullRequestNotOpened {
                                    issue,
                                    reason: refusal.to_string(),
                                });
                            }
                        }
                    }
                }
                // A later turn that rewrote the file changes the title and body,
                // and a project's hook may refuse what it wrote.
                let file_refused = match (handed_over, discovering, item.pull_request) {
                    (true, false, Some(number)) => self.edit_handed_over(&item.build, number),
                    _ => None,
                };
                let session = item.session.clone();
                let cost =
                    item.record_call(Role::Worker, now, session, reply.usage, reply.session_cost);
                item.summary = crate::board::briefing::summary(&reply.text);
                item.context = reply.context;
                // A fix turn's reply says what it made of each bot thread sent.
                let fixing = matches!(&item.phase, Phase::Review(r) if matches!(r.stage, ReviewStage::Fixing { .. }));
                if fixing {
                    item.answer_threads(&reply.text);
                }
                // A turn that left its work uncommitted and pushed nothing is
                // sent back once, before the gate can park it on a ruling. A
                // turn to push before a round is that once already.
                let asked = std::mem::take(&mut item.asked_to_commit);
                let pushing =
                    matches!(&item.phase, Phase::Review(r) if r.stage == ReviewStage::Pushing);
                // A worktree git cannot be read in is told and let go: the
                // turn ends as it did before, which errs toward the ruling.
                let uncommitted = match (question.is_none() && no_push && !asked && !pushing)
                    .then(|| worktree::uncommitted(&self.settings.git.checkout, &item.worktree))
                {
                    Some(Ok(files)) => files,
                    Some(Err(e)) => {
                        eprintln!("cannot list issue #{issue}'s uncommitted files: {e}");
                        Vec::new()
                    }
                    None => Vec::new(),
                };
                let commit_first = !uncommitted.is_empty();
                // The branch is handed over when the turn ends with it pushed,
                // asking nothing and holding no work back, and the project's
                // hooks judge it before review reads it. A refusal sends the
                // worker back once, and the next parks the work item.
                let ended = match commit_first || question.is_some() {
                    true => None,
                    false => self.turn_end(item, &reply.text),
                };
                let handed = match commit_first || question.is_some() || ended.is_some() {
                    true => Handover::NotDue,
                    false => self.handover(item),
                };
                let already = item.handover_refused;
                if let Handover::Passed(head) = &handed {
                    item.handed_over = Some(head.clone());
                    item.handover_refused = false;
                }
                let refusal = match (&ended, handed) {
                    (Some(why), _) => Some(Refused::turn(why)),
                    (None, Handover::Refused(why)) => Some(Refused::branch(&why)),
                    _ => file_refused.as_deref().map(Refused::pull_request_file),
                };
                // A turn no hook refused, with its work all committed, spent the
                // worker's send-back: the next refusal, of whatever hook, is
                // a new one and sends it back again.
                if refusal.is_none() && !commit_first {
                    item.handover_refused = false;
                }
                let hook_parks = refusal.clone().filter(|_| already);
                let hook_first = refusal.as_ref().filter(|_| !already);
                let to_worker = commit_first || refusal.is_some();
                item.turn = if to_worker {
                    item.asked_to_commit |= commit_first;
                    item.handover_refused |= hook_first.is_some();
                    // The pull request this turn opened still owes its first
                    // review, which the follow-up's end is too late to see as
                    // a discovery.
                    if discovering
                        && item.pull_request.is_some()
                        && item.resume.is_none()
                        && matches!(item.phase, Phase::Implement)
                    {
                        item.resume = Some(Phase::Review(Review::first()));
                    }
                    Turn::Next {
                        prompt: match &refusal {
                            Some(refused) if !commit_first => refused.prompt.clone(),
                            _ => uncommitted_prompt(&uncommitted),
                        },
                    }
                } else {
                    Turn::Ended { at: now }
                };
                // A question leaves the phase untouched: it interrupted
                // whatever was running, before that turn could be said to
                // have ended normally, and the answer resumes exactly this,
                // captured below before `park` parks it on a ruling.
                let stopped_short = pushed_nothing && awaits_a_push(item);
                if question.is_none() && !stopped_short && !to_worker {
                    if let Some(resume) = item.resume.take() {
                        item.phase = resume;
                        // The follow-up was the worker's one turn to push, so
                        // a round finding the worktree off the pushed head parks.
                        if asked
                            && let Phase::Review(review) = &mut item.phase
                            && review.stage == ReviewStage::Round
                        {
                            review.stage = ReviewStage::Pushing;
                        }
                    } else {
                        match item.phase.clone() {
                            Phase::Implement if discovering && item.pull_request.is_some() => {
                                item.phase = Phase::Review(Review::first());
                            }
                            // A fix sent to CI with no round's read, on purpose:
                            // red CI's, a conflict's.
                            Phase::Implement if item.pull_request.is_some() => {
                                let fixed = !no_push && own_commit;
                                if let Some(head) = item.known.head.clone().filter(|_| fixed) {
                                    item.send_unread(head);
                                }
                                item.phase = Phase::Ci {
                                    head: None,
                                    since: now,
                                };
                            }
                            // A fix turn stays fixing: the next step checks it pushed.
                            _ => {}
                        }
                    }
                }
                let session = item.session.clone();
                let (work_item_cost_usd, pull_request) = (item.cost().usd(), item.pull_request);
                match question {
                    // The hooks refused the branch again: the worker's turn to fix
                    // it was spent, so the work item parks, and a yes sends it back.
                    None if hook_parks.is_some() => {
                        let why = hook_parks.map(|refused| refused.parked).unwrap_or_default();
                        failed(self.names(), &mut next, issue, now, why)
                    }
                    None => StepReport::Ended {
                        issue,
                        session,
                        usage: reply.usage,
                        cost_usd: cost.map(Cost::usd),
                        work_item_cost_usd,
                        pull_request,
                    },
                    Some(text) => {
                        let resume = match &item.phase {
                            Phase::Review(review) => Resume::Review(review.clone()),
                            // A CI fix turn sent from a round waiting on its head's run
                            // goes back to that round once answered, and the answer
                            // keeps the saved round in `resume`.
                            Phase::Implement if matches!(item.resume, Some(Phase::Review(_))) => {
                                Resume::Nothing
                            }
                            // A merge ruling's no sends its fix to CI, question or none.
                            Phase::Implement if item.noted_from.is_some() => Resume::Nothing,
                            Phase::Implement if pull_request.is_some() => Resume::ReviewFirst,
                            _ => Resume::Nothing,
                        };
                        let kind = RulingKind::Question {
                            asked: text,
                            resume,
                        };
                        let names = self.names();
                        let (id, question) = park(names, &mut next, issue, pull_request, kind);
                        StepReport::Asked {
                            issue,
                            session,
                            usage: reply.usage,
                            cost_usd: cost.map(Cost::usd),
                            work_item_cost_usd,
                            pull_request,
                            id,
                            question,
                            comment_failed: None,
                        }
                    }
                }
            }
            Err(AgentError::TimedOut(_)) => {
                item.turn = Turn::Ended { at: now };
                timed_out(self.names(), &mut next, issue)
            }
            Err(e) => failed(self.names(), &mut next, issue, now, e.to_string()),
        };
        self.save(next)?;
        let mut report = report;
        self.fill_comment_failed(&mut report);
        Ok(Some(report))
    }

    // A ruling just raised is posted as a comment on its pull request, if it
    // has one; only these three reports carry a ruling and need the outcome.
    pub(super) fn fill_comment_failed(&mut self, report: &mut StepReport) {
        match report {
            StepReport::Asked {
                pull_request,
                id,
                comment_failed,
                ..
            }
            | StepReport::TimedOut {
                pull_request,
                id,
                comment_failed,
                ..
            }
            | StepReport::Failed {
                pull_request,
                id,
                comment_failed,
                ..
            } => {
                *comment_failed = self.post_ruling(*pull_request, *id);
            }
            _ => {}
        }
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

pub(super) fn write(folder: &Path, file: &Path, text: &str) -> Result<(), String> {
    let failed = |why: &dyn std::fmt::Display| format!("cannot write {}: {why}", file.display());
    let rel = file
        .strip_prefix(folder)
        .map_err(|_| failed(&"it is not in the folder"))?;
    worker_folder::write(folder, rel, text).map_err(|e| failed(&e))
}

#[cfg(test)]
mod tests;
