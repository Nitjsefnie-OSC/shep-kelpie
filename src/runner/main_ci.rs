//! Which workflows are red on `main`
//!
//! The runner reads the latest completed run of each workflow on `main` at
//! most once a [`MAIN_CI_POLL`](super::looks::MAIN_CI_POLL), and keeps the
//! workflows whose latest run failed in the project's state, for `status` and
//! the board to show. A workflow stays red until a run of it succeeds: a run
//! that was cancelled or skipped says nothing about the code, so it changes
//! nothing.

use serde::Serialize;

use super::Runner;
use crate::ports::{BranchRun, Timestamp};
use crate::state::{RedWorkflow, StateError};
use crate::worktree::BASE;

/// A workflow red on `main`, as `status` shows it
#[derive(Debug, Serialize)]
pub struct RedStatus {
    /// The workflow's name
    pub workflow: String,
    /// The forge's id for its first red run since it was last green
    pub first_run: u64,
    /// The forge's id for its latest red run
    pub run: u64,
    /// The commit the latest red run ran on
    pub head: String,
    /// When the first red run was created
    pub since: Timestamp,
    /// How long it has been red, in seconds
    pub red_for: u64,
    /// The latest red run's page on the forge
    pub url: String,
}

impl Runner {
    // Reads `main`'s workflows when they are due and the forge is not held, at
    // the end of a pass that acted on nothing else. A read the forge refuses waits
    // as any failed read does and leaves what the state holds.
    pub(super) fn read_main_ci(&mut self) -> Result<(), StateError> {
        let now = self.ports.clock.now();
        if !self.looks.main_ci_due(now) || self.forge_hold.until(now).is_some() {
            return Ok(());
        }
        let runs = self.ports.forge.branch_runs(&self.remote, BASE);
        self.looks
            .main_ci_read(now, runs.is_ok(), self.forge_hold.until(now));
        let Ok(runs) = runs else {
            return Ok(());
        };
        let red = still_red(&self.state.red, &runs);
        if red == self.state.red {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.red = red;
        self.save(next)?;
        self.board_changed();
        Ok(())
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

// `red` after `runs`, the latest completed run of each workflow: a red run
// keeps or adds its workflow, a successful one drops it, and any other
// leaves it as it was
fn still_red(red: &[RedWorkflow], runs: &[BranchRun]) -> Vec<RedWorkflow> {
    let mut next = red.to_vec();
    for run in runs {
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
