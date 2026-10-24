//! Reading `main`'s workflows with the runner's lock let go
//!
//! [`Runner::main_ci_job`] decides a read is due and copies what it needs, [`MainCiJob::run`]
//! reads, and [`Runner::apply_main_ci`] works the red workflows out again on the state it finds
//! and acts on them. A read the forge refuses waits as any failed read does and leaves what the
//! state holds; a state that cannot be saved is read again at the next poll.

use std::sync::{Arc, Mutex};

use super::{Read, still_red};
use crate::ports::{BranchRun, Forge, ForgeError, Workflow, WorkflowState};
use crate::runner::Runner;
use crate::runner::trigger::lock;
use crate::settings::ForgeSlug;
use crate::state::RedWorkflow;

/// Reads `main`'s workflows when they are due and the forge is not held, with the runner's lock
/// let go, at the start of every pass, before any work item is stepped
///
/// A step checks that it drains just before it saves the turn it starts, and a forge read between
/// that check and the call is time enough for a `drain` to land past it.
pub(in crate::runner) fn read_main_ci_off_lock(runner: &Mutex<Runner>) {
    let job = lock(runner).main_ci_job();
    let Some(job) = job else {
        return;
    };
    let read = job.run();
    lock(runner).apply_main_ci(read);
}

/// A read of `main`'s runs the runner decided was due, which needs no lock
struct MainCiJob {
    forge: Arc<dyn Forge>,
    repo: ForgeSlug,
    branch: String,
    /// The red workflows as they stood when the read began
    red: Vec<RedWorkflow>,
}

/// What a read found
struct Runs {
    runs: Vec<BranchRun>,
    /// The forge's workflows, read only while one was red
    workflows: Option<Vec<Workflow>>,
}

impl MainCiJob {
    // The latest runs on `main`, and the forge's workflows while something is red: a red
    // workflow stays listed when its latest run is older than the runs read, so the forge's list
    // is what drops one that can no longer run.
    fn run(self) -> Result<Runs, ForgeError> {
        let runs = self.forge.branch_runs(&self.repo, &self.branch)?;
        let workflows = match still_red(&self.red, &runs).red.is_empty() {
            true => None,
            false => Some(self.forge.workflows(&self.repo)?),
        };
        Ok(Runs { runs, workflows })
    }
}

impl Runner {
    // The read, when `main`'s workflows are due and the forge is not held
    fn main_ci_job(&self) -> Option<MainCiJob> {
        let now = self.ports.clock.now();
        if !self.looks.main_ci_due(now) || self.forge_hold.until(now).is_some() {
            return None;
        }
        Some(MainCiJob {
            forge: Arc::clone(&self.ports.forge),
            repo: self.remote.clone(),
            branch: self.settings.git.base().to_owned(),
            red: self.state.red.clone(),
        })
    }

    // Records the read and acts on what it found.
    fn apply_main_ci(&mut self, runs: Result<Runs, ForgeError>) {
        let now = self.ports.clock.now();
        let read = runs.and_then(|runs| self.red_after(runs));
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

    // The red workflows after `runs`, worked out on the state as it is now, which the forge's
    // list of workflows then thins.
    fn red_after(&self, runs: Runs) -> Result<Read, ForgeError> {
        let mut read = still_red(&self.state.red, &runs.runs);
        if read.red.is_empty() {
            return Ok(read);
        }
        let workflows = runs.workflows.ok_or_else(|| {
            ForgeError::Failed("a workflow went red while the workflows were not read".into())
        })?;
        read.red.retain(|r| {
            (workflows.iter()).any(|w| w.name == r.workflow && w.state == WorkflowState::Active)
        });
        Ok(read)
    }
}
