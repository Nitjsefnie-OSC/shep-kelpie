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
use crate::ports::{BranchRun, ForgeError, Timestamp, WorkflowState};
use crate::state::RedWorkflow;
use crate::worktree::BASE;

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
}

impl Runner {
    // Reads `main`'s workflows when they are due and the forge is not held, at
    // the end of every pass, so a work item that acts on each pass cannot keep
    // them from being read. A read the forge refuses waits as any failed read
    // does and leaves what the state holds; a state that cannot be saved is
    // read again at the next poll.
    pub(super) fn read_main_ci(&mut self) {
        let now = self.ports.clock.now();
        if !self.looks.main_ci_due(now) || self.forge_hold.until(now).is_some() {
            return;
        }
        let red = self.read_red();
        self.looks
            .main_ci_read(now, red.is_ok(), self.forge_hold.until(now));
        let Ok(red) = red else {
            return;
        };
        if red == self.state.red {
            return;
        }
        let mut next = self.state.clone();
        next.red = red;
        match self.save(next) {
            Ok(()) => self.board_changed(),
            Err(e) => eprintln!("cannot save the red workflows on main: {e}"),
        }
    }

    // The red workflows after the latest runs on `main`. A red workflow
    // stays listed when its latest run is older than the runs read, so the
    // forge's list of workflows is what drops one that can no longer run;
    // it is asked only while something is red.
    fn read_red(&self) -> Result<Vec<RedWorkflow>, ForgeError> {
        let runs = self.ports.forge.branch_runs(&self.remote, BASE)?;
        let mut red = still_red(&self.state.red, &runs);
        if !red.is_empty() {
            let workflows = self.ports.forge.workflows(&self.remote)?;
            red.retain(|r| {
                (workflows.iter()).any(|w| w.name == r.workflow && w.state == WorkflowState::Active)
            });
        }
        Ok(red)
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

// `red` after `runs`, the latest completed run of each workflow and event: a
// red run keeps or adds its workflow, a successful one drops it, and any
// other leaves it as it was
fn still_red(red: &[RedWorkflow], runs: &[BranchRun]) -> Vec<RedWorkflow> {
    let mut next = red.to_vec();
    for run in on_main(runs) {
        let at = next.iter().position(|r| r.workflow == run.workflow);
        if !is_red(&run.conclusion) {
            if let (Some(at), "success") = (at, run.conclusion.as_str()) {
                next.remove(at);
            }
        } else if let Some(at) = at {
            next[at].run = run.id;
            next[at].head.clone_from(&run.head);
        } else {
            next.push(RedWorkflow {
                workflow: run.workflow.clone(),
                first_run: run.id,
                run: run.id,
                head: run.head.clone(),
                since: run.created,
            });
        }
    }
    next
}

#[cfg(test)]
mod tests;
