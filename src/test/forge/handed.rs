//! The fake forge's pull requests kelpie opens for a worker, and the CI logs
//! it reads for one

use std::collections::HashMap;

use super::FakeForge;
use crate::ports::BranchRun;

/// What the fake forge's failed CI runs say, and the reruns asked of them
#[derive(Debug, Default)]
pub(super) struct CiRuns {
    log: String,
    log_error: Option<String>,
    reruns: Vec<String>,
    rerun_error: Option<String>,
    // The latest run of each workflow, by branch
    branch_runs: HashMap<String, Vec<BranchRun>>,
    // Why listing a branch's runs fails
    branch_runs_error: Option<String>,
    // The end of each run's failed log, by run id
    run_logs: HashMap<u64, String>,
}

impl CiRuns {
    /// The logs, or why reading them fails
    pub(super) fn log(&self) -> Result<String, String> {
        match &self.log_error {
            Some(error) => Err(error.clone()),
            None => Ok(self.log.clone()),
        }
    }

    /// Records a rerun of `head`, or says why it fails
    pub(super) fn rerun(&mut self, head: &str) -> Result<(), String> {
        match &self.rerun_error {
            Some(error) => Err(error.clone()),
            None => {
                self.reruns.push(head.to_owned());
                Ok(())
            }
        }
    }
}

impl FakeForge {
    /// Makes the failed CI runs' logs say `log`
    pub(crate) fn set_ci_log(&self, log: &str) {
        let mut ci = self.ci.lock().unwrap();
        (ci.log, ci.log_error) = (log.to_owned(), None);
    }

    /// Makes reading the failed CI runs' logs fail with `error`, verbatim
    pub(crate) fn set_ci_log_error(&self, error: &str) {
        self.ci.lock().unwrap().log_error = Some(error.to_owned());
    }

    /// The heads whose failed CI jobs kelpie reran, in order
    pub(crate) fn reruns(&self) -> Vec<String> {
        self.ci.lock().unwrap().reruns.clone()
    }

    /// Makes rerunning failed CI jobs fail with `error`, verbatim
    pub(crate) fn set_rerun_error(&self, error: &str) {
        self.ci.lock().unwrap().rerun_error = Some(error.to_owned());
    }

    /// Makes `branch` list `runs`, the latest completed run of each of its workflows
    pub(crate) fn set_branch_runs(&self, branch: &str, runs: Vec<BranchRun>) {
        self.ci
            .lock()
            .unwrap()
            .branch_runs
            .insert(branch.to_owned(), runs);
    }

    /// Makes listing a branch's runs fail with `error`, verbatim
    pub(crate) fn set_branch_runs_error(&self, error: &str) {
        self.ci.lock().unwrap().branch_runs_error = Some(error.to_owned());
    }

    /// Makes the failed log of run `run` end in `tail`
    pub(crate) fn set_run_log_tail(&self, run: u64, tail: &str) {
        self.ci
            .lock()
            .unwrap()
            .run_logs
            .insert(run, tail.to_owned());
    }

    /// The runs `branch` lists: none for a branch no test scripted
    pub(super) fn branch_runs_of(&self, branch: &str) -> Result<Vec<BranchRun>, String> {
        let ci = self.ci.lock().unwrap();
        match &ci.branch_runs_error {
            Some(error) => Err(error.clone()),
            None => Ok(ci.branch_runs.get(branch).cloned().unwrap_or_default()),
        }
    }

    /// The end of run `run`'s failed log, or why none can be read
    pub(super) fn run_log_tail_of(&self, run: u64) -> Result<String, String> {
        let ci = self.ci.lock().unwrap();
        ci.run_logs
            .get(&run)
            .cloned()
            .ok_or_else(|| format!("no log for run {run}"))
    }

    /// Makes pull request `number` read with `title`
    pub(crate) fn set_title(&self, number: u64, title: &str) {
        self.titles.lock().unwrap().insert(number, title.to_owned());
    }

    /// The title and body pull request `number` was opened or edited with
    pub(crate) fn pull_request_text(&self, number: u64) -> Option<(String, String)> {
        self.texts.lock().unwrap().get(&number).cloned()
    }

    /// The title and body kelpie gave pull request `number` when it opened or
    /// edited it, or none when it did neither
    pub(crate) fn text_of(&self, number: u64) -> Option<(String, String)> {
        self.texts.lock().unwrap().get(&number).cloned()
    }
}
