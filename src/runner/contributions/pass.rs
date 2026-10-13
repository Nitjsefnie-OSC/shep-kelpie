//! The pass of the reviewers over a contribution's green head
//!
//! No reviewer reads a head until it has passed the security review and CI
//! on it is green (ADR 0008). Then one pass walks `contributions.reviewers`
//! from the reviewer after the last that answered, wrapping round, and the
//! first to answer, with findings or `CLEAN`, ends it. The next push starts
//! its pass there, so the reviewers take turns over a contribution's
//! pushes. A reviewer limited to `paths` the change leaves alone is skipped,
//! as is a local one that has reviewed nothing twice and, until bots are
//! summoned here, every bot. One whose call fails three times is passed over.
//! A pass no reviewer read ends unread, with why.
//!
//! The pass is a call in flight like the security review, sharing its
//! one place, and its reviewers read a detached worktree of the fetched
//! `refs/pull/<n>/head` under the project's folder, with read tools only,
//! once the agents' own files in it are checked against `main`'s. Nothing is
//! posted. The worktree stays until the pull request leaves the record.

use std::path::PathBuf;

use super::super::Runner;
use super::super::report::Begin;
use super::super::review::calls::{build_call, diff_against};
use super::super::review::{ROUND_FAILURES, changed_files, prompts, touches};
use super::super::untrusted;
use super::Job as Call;
use super::security::MAX_DIFF;
use crate::fence;
use crate::ports::{
    AgentError, Agents, Cost, Ending, Finding, Reviewer, ReviewerError, Role, Tools, read_review,
};
use crate::settings::{AgentName, ListedReviewer};
use crate::state::{Pass, StateError};
use crate::work_item::LOCAL_FAILURES_DOWN;
use crate::worktree::{self, BASE, Linked};

/// The folder under the project's folder where each contribution's pass is
/// checked out, one folder each
const FOLDER: &str = "reading";

/// One pass to run, with everything it needs, taken while the runner's lock
/// was held so that the pass itself runs without it
pub(in crate::runner) struct Job {
    pub(in crate::runner) pull_request: u64,
    pub(in crate::runner) head: String,
    repo: PathBuf,
    worktree: PathBuf,
    out: PathBuf,
    settings: PathBuf,
    // The reviewers that can run, in the order the pass walks them
    reviewers: Vec<ListedReviewer>,
    // Why each listed one that cannot run is passed over
    passed_over: Vec<String>,
}

/// What a pass came to
#[derive(Debug, Default)]
pub(in crate::runner) struct Walked {
    /// The pass, or `None` when it could not be had (the head cannot be
    /// fetched or checked out, or the runner stopped it), which leaves the
    /// head to be reviewed at the next poll
    pub(in crate::runner) pass: Option<Pass>,
    // The local reviewers that reviewed something, and those that reviewed
    // nothing
    reviewed: Vec<AgentName>,
    nothing: Vec<AgentName>,
    /// What the calls cost, where they came back priced
    pub(in crate::runner) cost: Cost,
}

// How one reviewer's turn in the pass ended
enum Turn {
    // Its answer: findings, or none for `CLEAN`
    Answered(Vec<Finding>),
    // A local reviewer that reviewed no file
    Blank,
    // Its calls failed `ROUND_FAILURES` times, the last for this reason
    Failed(String),
    // The runner stopped the call
    Stopped,
}

impl Runner {
    // The pass over the current head of one contribution that has passed the
    // security review and whose CI is green, as the call to start, unless a
    // call on a contribution is in flight or the runner drains. One is tried
    // once a poll, and a contribution over its cap starts none.
    //
    // The caller holds a free slot for it, which the call keeps until it ends.
    pub(in crate::runner) fn run_pass(&mut self) -> Result<Option<Begin>, StateError> {
        if self.draining || self.flights.securing() {
            return Ok(None);
        }
        let due: Vec<_> = (self.state.contributions.iter())
            .filter(|c| c.is_passed() && c.is_green() && !c.is_reviewed())
            .filter(|c| !self.flights.security_tried(c.pull_request))
            .map(|c| c.pull_request)
            .collect();
        for number in due {
            if self.over_budget(number)? {
                continue;
            }
            return Ok(self
                .pass_job(number)
                .map(|job| Begin::Secure(Call::Pass(job))));
        }
        Ok(None)
    }

    fn pass_job(&self, number: u64) -> Option<Job> {
        let contribution = (self.state.contributions.iter()).find(|c| c.pull_request == number)?;
        let lineup = &self.contribution_lineup;
        let start = (contribution.last_answered.as_ref())
            .and_then(|last| lineup.iter().position(|r| &r.name == last))
            .map_or(0, |at| at + 1);
        let walk = (0..lineup.len()).map(|i| &lineup[(start + i) % lineup.len()]);
        let mut reviewers = Vec::new();
        let mut passed_over = Vec::new();
        for r in walk {
            let failures = contribution.local_failures.get(&r.name).copied();
            if r.bot().is_some() {
                passed_over.push(format!("{} is a bot, out of its window here", r.name));
            } else if r.is_local() && failures.is_some_and(|n| n >= LOCAL_FAILURES_DOWN) {
                passed_over.push(format!("{} reviewed no file twice", r.name));
            } else {
                reviewers.push(r.clone());
            }
        }
        let folder = self.paths.folder.join(FOLDER);
        Some(Job {
            pull_request: number,
            head: contribution.head.clone(),
            repo: self.settings.git.checkout.clone(),
            worktree: folder.join(number.to_string()),
            out: folder.join(format!("{number}-out")),
            settings: (self.paths.worker).join(format!("pass-settings-{number}.json")),
            reviewers,
            passed_over,
        })
    }

    // Records what the pass over `pull_request`'s `head` came to: its cost
    // goes to the contribution's spend, and the pass is recorded unless the
    // contribution moved on to another head or is gone. The reviewer that
    // answered is the one the next pass starts after.
    pub(in crate::runner) fn record_pass(
        &mut self,
        pull_request: u64,
        head: &str,
        walked: Walked,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == pull_request)
        else {
            return Ok(());
        };
        held.spend = Cost(held.spend.0.saturating_add(walked.cost.0));
        let Some(pass) = walked.pass.filter(|_| held.head == head) else {
            return match walked.cost.is_free() {
                true => Ok(()),
                false => self.save(next),
            };
        };
        for name in &walked.reviewed {
            held.local_failures.remove(name);
        }
        for name in walked.nothing {
            *held.local_failures.entry(name).or_default() += 1;
        }
        if pass.answered.is_some() {
            held.last_answered.clone_from(&pass.answered);
        }
        held.pass = Some(pass);
        self.save(next)
    }

    // Removes the checkout of each pull request in `before` that the state
    // no longer follows. A removal that fails is logged: the next pass over
    // the pull request, if it comes back, replaces what is there.
    pub(super) fn remove_checkouts(&self, before: &[u64]) {
        let folder = self.paths.folder.join(FOLDER);
        let gone = before
            .iter()
            .filter(|n| (self.state.contributions.iter()).all(|c| c.pull_request != **n));
        for number in gone {
            let path = folder.join(number.to_string());
            if let Err(e) = worktree::remove_view(&self.settings.git.checkout, &path) {
                eprintln!("contribution #{number}: cannot remove its checkout: {e}");
            }
            let _ = std::fs::remove_dir_all(folder.join(format!("{number}-out")));
        }
    }
}

impl Job {
    // Walks the reviewers: the pass, or none when the head could not be had
    // or the runner stopped the pass, which leaves it for the next poll.
    pub(in crate::runner) fn walk(
        self,
        agents: &dyn Agents,
        reviewer: &dyn Reviewer,
        ending: &Ending,
    ) -> Walked {
        let number = self.pull_request;
        let fail = |why: String| {
            eprintln!("contribution #{number}: the pass left it unreviewed: {why}");
            Walked::default()
        };
        let checkout =
            worktree::contribution_checkout(&self.repo, number, &self.head, &self.worktree);
        if let Err(e) = checkout {
            return fail(format!("cannot check out {}: {e}", self.head));
        }
        // A harness loads its own files from the folder it runs in, and they
        // run code, so a head that changes them is read by no one.
        match fence::differ(&self.repo, &self.worktree, None) {
            Ok(files) if files.is_empty() => {}
            Ok(files) => {
                let why = format!(
                    "the head changes agents' own files, which no reviewer runs beside: {}",
                    files.join(", ")
                );
                return self.ended(Vec::new(), None, vec![why], Walked::default());
            }
            Err(e) => return fail(format!("cannot check its agents' own files: {e}")),
        }
        let base = format!("origin/{BASE}");
        let changed = match (self.reviewers.iter().any(|r| !r.paths.is_empty()))
            .then(|| changed_files(&self.repo, &self.worktree, &base))
        {
            Some(Err(e)) => return fail(e),
            Some(Ok(changed)) => changed,
            None => Vec::new(),
        };
        let mut walked = Walked::default();
        let mut ran = Vec::new();
        let mut missed = self.passed_over.clone();
        let diff = OnceDiff::new(&self, &base);
        for r in &self.reviewers {
            if !touches(&r.paths, &changed) {
                missed.push(format!(
                    "{} reads only paths this change leaves alone",
                    r.name
                ));
                continue;
            }
            ran.push(r.name.clone());
            let turn = self.turn(r, &diff, agents, reviewer, ending, &mut walked.cost);
            match turn {
                Turn::Stopped => return Walked::default(),
                Turn::Answered(findings) => {
                    if r.is_local() {
                        walked.reviewed.push(r.name.clone());
                    }
                    return self.ended(ran, Some((r.name.clone(), findings)), missed, walked);
                }
                Turn::Blank => {
                    walked.nothing.push(r.name.clone());
                    missed.push(format!("{} reviewed no file", r.name));
                }
                Turn::Failed(why) => {
                    missed.push(format!(
                        "{} was passed over after its calls kept failing: {why}",
                        r.name
                    ));
                }
            }
        }
        self.ended(ran, None, missed, walked)
    }

    // The pass that ran `ran`, which `answer` ended, or that ended unread for
    // the reasons in `missed`.
    fn ended(
        &self,
        ran: Vec<AgentName>,
        answer: Option<(AgentName, Vec<Finding>)>,
        missed: Vec<String>,
        walked: Walked,
    ) -> Walked {
        let unread = match answer {
            Some(_) => None,
            None if missed.is_empty() => Some("no reviewer is listed".to_owned()),
            None => Some(missed.join(", ")),
        };
        let (answered, findings) = match answer {
            Some((name, findings)) => (Some(name), findings),
            None => (None, Vec::new()),
        };
        Walked {
            pass: Some(Pass {
                head: self.head.clone(),
                ran,
                answered,
                unread,
                findings,
            }),
            ..walked
        }
    }

    // One reviewer's turn: its call, tried up to `ROUND_FAILURES` times.
    fn turn(
        &self,
        r: &ListedReviewer,
        diff: &OnceDiff,
        agents: &dyn Agents,
        reviewer: &dyn Reviewer,
        ending: &Ending,
        cost: &mut Cost,
    ) -> Turn {
        let mut why = String::new();
        for _ in 0..ROUND_FAILURES {
            let asked = match r.runs.local() {
                Some(local) => self.local_round(&local, reviewer),
                None => self.session(r, diff, agents, ending, cost),
            };
            match asked {
                Ok(turn) => return turn,
                Err(Stop::Stopped) => return Turn::Stopped,
                Err(Stop::Failed(reason)) => {
                    eprintln!(
                        "contribution #{}: {} failed: {reason}",
                        self.pull_request, r.name
                    );
                    why = reason;
                }
            }
        }
        Turn::Failed(why)
    }

    fn local_round(
        &self,
        local: &crate::settings::LocalRound,
        reviewer: &dyn Reviewer,
    ) -> Result<Turn, Stop> {
        let linked = Linked {
            repo: &self.repo,
            worktree: &self.worktree,
        };
        let base = format!("origin/{BASE}");
        let found = reviewer.round_watched(local, linked, &base, &self.out, 1, "", &|_| {});
        match found {
            Err(ReviewerError::Stopped) => Err(Stop::Stopped),
            Err(e) => Err(Stop::Failed(e.to_string())),
            Ok(found) => {
                // A line for a file the script could not review is no finding.
                let (unreviewed, findings): (Vec<_>, Vec<_>) =
                    found.into_iter().partition(|f| f.is_unreviewed());
                match findings.is_empty() && !unreviewed.is_empty() {
                    true => Ok(Turn::Blank),
                    false => Ok(Turn::Answered(findings)),
                }
            }
        }
    }

    fn session(
        &self,
        r: &ListedReviewer,
        diff: &OnceDiff,
        agents: &dyn Agents,
        ending: &Ending,
        cost: &mut Cost,
    ) -> Result<Turn, Stop> {
        let Some((model, _)) = r.runs.session() else {
            return Err(Stop::Failed("it runs no session".to_owned()));
        };
        let diff = diff.get().map_err(Stop::Failed)?;
        let body = r.prompt.as_deref().unwrap_or_default();
        let base = format!("origin/{BASE}");
        let prompt = prompts::reviewer_prompt(
            body,
            &base,
            &untrusted::fenced("The diff", &diff),
            "",
            None,
            None,
        );
        let mut call = build_call(
            Role::Reviewer,
            self.pull_request,
            &self.worktree,
            model,
            prompt,
        )
        .map_err(Stop::Failed)?;
        call.settings.clone_from(&self.settings);
        call.tools = Tools::Review;
        let ran = agents
            .prepare(&call)
            .and_then(|()| agents.run(&call, ending));
        let _ = std::fs::remove_file(&call.settings);
        let reply = match ran {
            Ok(reply) => reply,
            Err(AgentError::Stopped) => return Err(Stop::Stopped),
            Err(e) => return Err(Stop::Failed(e.to_string())),
        };
        // A fresh session, so what it has cost is what this call did.
        let spent = reply.session_cost.unwrap_or_default();
        *cost = Cost(cost.0.saturating_add(spent.0));
        read_review(&reply.text)
            .map(Turn::Answered)
            .map_err(|said| Stop::Failed(format!("its reply is no verdict: {said:?}")))
    }
}

// Why a call did not come to an answer
enum Stop {
    Stopped,
    Failed(String),
}

// The head's diff from `main`, read once and only if a session reviewer asks,
// cut at `MAX_DIFF` bytes
struct OnceDiff<'a> {
    job: &'a Job,
    base: &'a str,
    read: std::cell::OnceCell<Result<String, String>>,
}

impl<'a> OnceDiff<'a> {
    fn new(job: &'a Job, base: &'a str) -> Self {
        Self {
            job,
            base,
            read: std::cell::OnceCell::new(),
        }
    }

    fn get(&self) -> Result<String, String> {
        let read = self.read.get_or_init(|| {
            let diff = diff_against(&self.job.repo, &self.job.worktree, self.base)?;
            Ok(cut(diff))
        });
        read.clone()
    }
}

// `diff` cut to `MAX_DIFF` bytes at a character boundary, with a line saying so
fn cut(mut diff: String) -> String {
    if diff.len() <= MAX_DIFF {
        return diff;
    }
    let mut at = MAX_DIFF;
    while !diff.is_char_boundary(at) {
        at -= 1;
    }
    diff.truncate(at);
    diff.push_str("\n(the diff is cut here: read the rest in the worktree)");
    diff
}
