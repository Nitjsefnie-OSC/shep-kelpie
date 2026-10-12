//! A failing test a reviewer attaches to a finding
//!
//! With `ci.test_command` set, a reviewer on Claude Code may write a failing
//! test for a finding into `<build>/review-tests`, at the path the test should
//! have in the repo, and name that path as the finding's fifth field. The
//! reviewer can write nowhere else. When the round's findings are taken, each
//! attached file is checked, and a checked one is copied to
//! `<worker>/review-tests-<issue>`, a folder the worker cannot write: the
//! original stays in the build folder for the worker to read, and the copy is
//! what a later step runs, so the worker cannot change it.

use std::io;
use std::path::{Path, PathBuf};

use super::super::Runner;
use crate::ports::{Finding, Reach};
use crate::settings::{AgentName, Harness, RoleModel, TestCommand};
use crate::worker_folder;

/// The folder in the build folder where a reviewer writes its tests
pub(in crate::runner) const TESTS_FOLDER: &str = "review-tests";

/// Where a reviewer writes the tests for a work item whose build folder is `build`
pub(in crate::runner) fn tests_folder(build: &Path) -> PathBuf {
    build.join(TESTS_FOLDER)
}

/// Where the checked copies of issue `issue`'s tests are kept, under the
/// worker folder
pub(in crate::runner) fn kept_folder(worker: &Path, issue: u64) -> PathBuf {
    worker.join(format!("{TESTS_FOLDER}-{issue}"))
}

impl Runner {
    /// The project's test command, when a reviewer on `model` is asked for tests:
    /// only a Claude Code session, whose writes the fence can hold to a folder
    pub(super) fn test_command_for(&self, model: &RoleModel) -> Option<&TestCommand> {
        let command = self.settings.ci.test_command.as_ref();
        command.filter(|_| model.harness.harness() == Harness::ClaudeCode)
    }

    /// Readies the folders for a session round, and gives a reviewer asked for
    /// tests the reach that lets it write its own folder alone
    ///
    /// A first look starts both folders empty. A second look finds the first's
    /// files where they are.
    ///
    /// # Errors
    ///
    /// Why a folder could not be cleared or made.
    pub(super) fn begin_tests(
        &self,
        model: &RoleModel,
        (issue, worktree, build): (u64, &Path, &Path),
        first_look: bool,
    ) -> Result<Option<Reach>, String> {
        let (written, kept) = (tests_folder(build), kept_folder(&self.paths.worker, issue));
        if first_look {
            for folder in [&written, &kept] {
                match std::fs::remove_dir_all(folder) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => {
                        return Err(format!("cannot clear {}: {e}", folder.display()));
                    }
                    _ => {}
                }
            }
        }
        if self.test_command_for(model).is_none() {
            return Ok(None);
        }
        std::fs::create_dir_all(&written)
            .map_err(|e| format!("cannot make {}: {e}", written.display()))?;
        let code = [worktree.to_owned()];
        Ok(Some(self.fenced_reach(
            &written,
            &code,
            vec![written.clone()],
            Vec::new(),
        )))
    }

    /// Whether the round `reviewer` ran asked it for tests
    pub(super) fn round_asked_tests(&self, reviewer: &AgentName) -> bool {
        let listed = self.listed(reviewer);
        let model = listed.as_ref().and_then(|r| r.runs.session());
        model.is_some_and(|(model, _)| self.test_command_for(model).is_some())
    }

    /// `found` with each attached test checked: a regular file in the reviewer's
    /// folder that the worktree has no file of that name for, which is then
    /// copied to the worker folder's
    ///
    /// A finding that fails keeps its text and loses `test`, and a note says
    /// why. When the round was not `asked` for tests, a fifth field was never
    /// one, so it goes back into `why` as an older kelpie read it.
    pub(super) fn checked_tests(
        &self,
        asked: bool,
        (issue, worktree, build): (u64, &Path, &Path),
        found: Vec<Finding>,
    ) -> Vec<Finding> {
        let (written, kept) = (tests_folder(build), kept_folder(&self.paths.worker, issue));
        let mut checked = Vec::with_capacity(found.len());
        for mut finding in found {
            let Some(test) = finding.test.take() else {
                checked.push(finding);
                continue;
            };
            if !asked {
                finding.why = format!("{}|{test}", finding.why);
            } else {
                match keep(&written, &kept, worktree, &test) {
                    Ok(()) => finding.test = Some(test),
                    Err(reason) => self.notes.push(format!(
                        "issue #{issue}: the test {test} for {}:{} is dropped and the finding \
                         stays: {reason}",
                        finding.file, finding.line
                    )),
                }
            }
            checked.push(finding);
        }
        checked
    }
}

// Checks `test` below `written` and copies it below `kept`.
fn keep(written: &Path, kept: &Path, worktree: &Path, test: &str) -> Result<(), String> {
    let rel = Path::new(test);
    let text = worker_folder::read(written, rel)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "the reviewer left no such file".to_owned())?;
    // A link there is as much the worktree's as a file.
    match worktree.join(rel).symlink_metadata() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => return Err("the worktree already holds a file of that name".to_owned()),
        Err(e) => return Err(format!("cannot look for it in the worktree: {e}")),
    }
    worker_folder::write(kept, rel, &text).map_err(|e| format!("cannot keep a copy: {e}"))
}
