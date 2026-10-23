//! Which workflows are red on `main`
//!
//! The runner reads the latest completed run of each workflow on `main` at
//! most once a [`MAIN_CI_POLL`](super::looks::MAIN_CI_POLL), and keeps the
//! workflows whose latest run failed in the project's state, for `status` and
//! the board to show. A workflow stays red until a run of it succeeds: a run
//! that was cancelled or skipped says nothing about the code, so it changes
//! nothing. A workflow the forge no longer runs, because its file is gone or
//! it is disabled, drops out whatever its last run was.

use serde::Serialize;

use super::Runner;
use crate::ports::{BranchRun, ForgeError, Rerun, Timestamp, WorkflowState};
use crate::state::{MainNotice, Notice, RedWorkflow};

/// A workflow red on `main`, as `status` shows it
#[derive(Debug, Serialize)]
pub struct RedStatus {
    /// The workflow's name
    pub workflow: String,
    /// The forge's id for the first red run kelpie read since it last read the
    /// workflow green
    pub first_run: u64,
    /// The forge's id for its latest red run
    pub run: u64,
    /// The commit the latest red run ran on
    pub head: String,
    /// When `first_run` was created
    pub since: Timestamp,
    /// How long it has been red, in seconds
    pub red_for: u64,
    /// The latest red run's page on the forge
    pub url: String,
    /// What is being done about the latest red head
    pub doing: String,
}

impl Runner {
    // Reads `main`'s workflows when they are due and the forge is not held, at
    // the start of every pass, before any work item is stepped. A step checks
    // that it drains just before it saves the turn it starts, and a forge read
    // between that check and the call is time enough for a `drain` to land past
    // it. A read the forge refuses waits as any failed read does and leaves what
    // the state holds; a state that cannot be saved is read again at the next
    // poll.
    pub(super) fn read_main_ci(&mut self) {
        let now = self.ports.clock.now();
        if !self.looks.main_ci_due(now) || self.forge_hold.until(now).is_some() {
            return;
        }
        let read = self.read_red();
        self.looks
            .main_ci_read(now, read.is_ok(), self.forge_hold.until(now));
        let Ok(read) = read else {
            return;
        };
        let mut next = self.state.clone();
        next.red = read.red;
        let notices = self.act_on_reds(&read.failing, &mut next.red, &read.cleared, now);
        next.notices.extend(notices);
        if next.red == self.state.red {
            return;
        }
        match self.save(next) {
            Ok(()) => self.board_changed(),
            Err(e) => eprintln!("cannot save the red workflows on main: {e}"),
        }
    }

    // The red workflows after the latest runs on `main`. A red workflow
    // stays listed when its latest run is older than the runs read, so the
    // forge's list of workflows is what drops one that can no longer run;
    // it is asked only while something is red.
    fn read_red(&self) -> Result<Read, ForgeError> {
        let runs = self
            .ports
            .forge
            .branch_runs(&self.remote, self.settings.git.base())?;
        let mut read = still_red(&self.state.red, &runs);
        if !read.red.is_empty() {
            let workflows = self.ports.forge.workflows(&self.remote)?;
            read.red.retain(|r| {
                (workflows.iter()).any(|w| w.name == r.workflow && w.state == WorkflowState::Active)
            });
        }
        Ok(read)
    }

    // What to do about the workflows this read found red, and the notices it
    // leaves to post. A workflow in `failing` has the failed jobs of each new
    // red head rerun once, and is told once for the stretch when red again
    // after a rerun, or at once where the forge refuses one. A red-again head
    // is the project manager's to fix or ask about. A limit that is used up
    // waits for the next read. Each of `cleared` that was told is told green.
    fn act_on_reds(
        &self,
        failing: &[String],
        red: &mut [RedWorkflow],
        cleared: &[Cleared],
        now: Timestamp,
    ) -> Vec<Notice> {
        let mut notices = Vec::new();
        for workflow in red.iter_mut().filter(|r| failing.contains(&r.workflow)) {
            if workflow.rerun_done() {
                workflow.again = true;
            } else {
                match self
                    .ports
                    .forge
                    .rerun_run(&self.remote, workflow.run, Rerun::Failed)
                {
                    Ok(()) => {
                        workflow.reran = Some(workflow.run);
                        workflow.reran_head = Some(workflow.head.clone());
                        continue;
                    }
                    Err(ForgeError::Held(_) | ForgeError::Limited { .. }) => continue,
                    Err(e) => {
                        eprintln!("cannot rerun {} on main: {e}", workflow.workflow);
                        workflow.reran_head = Some(workflow.head.clone());
                        workflow.again = true;
                    }
                }
            }
            if workflow.noticed {
                continue;
            }
            workflow.noticed = true;
            notices.push(Notice::Main(MainNotice {
                workflow: workflow.workflow.clone(),
                run: workflow.run,
                red_for: now.0.saturating_sub(workflow.since.0),
                log: self.log_end(workflow.run),
                green: false,
            }));
        }
        for Cleared { was, by } in cleared.iter().filter(|c| c.was.noticed) {
            notices.push(Notice::Main(MainNotice {
                workflow: was.workflow.clone(),
                run: *by,
                red_for: now.0.saturating_sub(was.since.0),
                log: String::new(),
                green: true,
            }));
        }
        notices
    }

    // The last lines of run `run`'s failed log, or none where it cannot be read
    fn log_end(&self, run: u64) -> String {
        let log = self.ports.forge.run_log_tail(&self.remote, run);
        let log = log.unwrap_or_else(|e| {
            eprintln!("cannot read the log of run {run} on main: {e}");
            String::new()
        });
        let lines: Vec<&str> = log.lines().collect();
        let end = lines[lines.len().saturating_sub(NOTICE_LOG_LINES)..].join("\n");
        let mut cut = end.len().saturating_sub(NOTICE_LOG_BYTES);
        while !end.is_char_boundary(cut) {
            cut += 1;
        }
        end[cut..].to_owned()
    }

    /// The workflows red on `main` as `status` shows them, in the order they went red
    pub(super) fn red_status(&self, now: Timestamp) -> Vec<RedStatus> {
        (self.state.red.iter())
            .map(|red| RedStatus {
                workflow: red.workflow.clone(),
                first_run: red.first_run,
                run: red.run,
                head: red.head.clone(),
                since: red.since,
                red_for: now.0.saturating_sub(red.since.0),
                doing: red.doing(),
                url: format!(
                    "https://github.com/{}/actions/runs/{}",
                    self.remote.as_str(),
                    red.run
                ),
            })
            .collect()
    }
}

// Whether a run ended in a way that means the code is broken
fn is_red(conclusion: &str) -> bool {
    matches!(conclusion, "failure" | "timed_out" | "startup_failure")
}

// The newest run of each workflow among those a push, a schedule or a manual
// start made, in the order the workflows first appear. A pull request's run
// is not `main`'s: one from a fork's own `main` carries the branch's name.
fn on_main(runs: &[BranchRun]) -> Vec<&BranchRun> {
    let mut latest: Vec<&BranchRun> = Vec::new();
    for run in runs
        .iter()
        .filter(|r| !matches!(r.event.as_str(), "pull_request" | "pull_request_target"))
    {
        match latest.iter_mut().find(|r| r.workflow == run.workflow) {
            Some(kept) if kept.created < run.created => *kept = run,
            Some(_) => {}
            None => latest.push(run),
        }
    }
    latest
}

/// How much of the end of a failed log a notice carries: at most this many
/// lines, and at most this many bytes, which a webhook's message has room for
const NOTICE_LOG_LINES: usize = 20;
const NOTICE_LOG_BYTES: usize = 2000;

/// What a read of `main`'s runs found
struct Read {
    /// The red workflows after it
    red: Vec<RedWorkflow>,
    /// The workflows whose latest run it read was red
    failing: Vec<String>,
    /// The workflows a successful run took out of `red`
    cleared: Vec<Cleared>,
}

/// A workflow that went green
struct Cleared {
    /// How it stood when it was red
    was: RedWorkflow,
    /// The run that cleared it
    by: u64,
}

// `red` after `runs`, the latest completed run of each workflow and event: a
// red run keeps or adds its workflow, a successful one drops it, and any
// other leaves it as it was. A red workflow's run from before the first one
// kelpie read, or before the one it reran, is not its latest: while the
// rerun's jobs go the run is not completed, and an older run shows in its
// place. The forge numbers runs as they are made.
fn still_red(red: &[RedWorkflow], runs: &[BranchRun]) -> Read {
    let mut next = red.to_vec();
    let mut failing = Vec::new();
    let mut cleared = Vec::new();
    for run in on_main(runs) {
        let at = next.iter().position(|r| r.workflow == run.workflow);
        let older =
            |r: &RedWorkflow| run.created < r.since || r.reran.is_some_and(|rerun| run.id < rerun);
        if at.is_some_and(|at| older(&next[at])) {
            continue;
        }
        if !is_red(&run.conclusion) {
            if let (Some(at), "success") = (at, run.conclusion.as_str()) {
                cleared.push(Cleared {
                    was: next.remove(at),
                    by: run.id,
                });
            }
            continue;
        }
        failing.push(run.workflow.clone());
        if let Some(at) = at {
            let red = &mut next[at];
            if red.head != run.head {
                red.again = false;
                red.handled = None;
            }
            red.run = run.id;
            red.head.clone_from(&run.head);
        } else {
            next.push(RedWorkflow {
                workflow: run.workflow.clone(),
                first_run: run.id,
                run: run.id,
                head: run.head.clone(),
                since: run.created,
                reran: None,
                reran_head: None,
                again: false,
                handled: None,
                noticed: false,
            });
        }
    }
    Read {
        red: next,
        failing,
        cleared,
    }
}

#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod tests;
