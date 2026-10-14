//! A failing test a reviewer attaches to a finding
//!
//! With `ci.test_command` set, a reviewer on any harness may write a failing
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

/// The folder in the build folder a Codex or goose reviewer with no tests to write is fenced to
///
/// It stays empty: the reviewer reads the worktree and writes nothing.
pub(in crate::runner) const READING_FOLDER: &str = "review-read";

/// The tool a reviewer on `harness` writes its test with, or `None` when the
/// fence cannot hold that harness's writes to a folder
pub(in crate::runner) fn writing_tool(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::ClaudeCode => Some("Write"),
        Harness::Pi => Some("write"),
        Harness::Codex => Some("apply_patch"),
        Harness::Goose => Some("write"),
        #[cfg(test)]
        Harness::StandIn => None,
    }
}

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
    /// a session on a harness whose writes the fence can hold to a folder
    pub(super) fn test_command_for(&self, model: &RoleModel) -> Option<&TestCommand> {
        let command = self.settings.ci.test_command.as_ref();
        command.filter(|_| writing_tool(model.harness.harness()).is_some())
    }

    /// Readies the folders for a session round, and gives a reviewer asked for
    /// tests the reach that lets it write its own folder alone
    ///
    /// A Codex or goose reviewer not asked for tests gets a fence too, with no
    /// write folder, since its only way to read the worktree is a shell that
    /// the fence's guard judges. A reviewer on any other harness has none.
    ///
    /// A first look of a reviewer asked for tests starts its own folder empty. A
    /// second look finds the first's files where they are. The checked copies
    /// stay across reviewers and passes: a held finding still names its file,
    /// and a path kept again is written over.
    ///
    /// # Errors
    ///
    /// Why a folder could not be cleared or made.
    pub(super) fn begin_tests(
        &self,
        model: &RoleModel,
        (worktree, build): (&Path, &Path),
        first_look: bool,
    ) -> Result<Option<Reach>, String> {
        if self.test_command_for(model).is_none() {
            return self.begin_reading(model, worktree, build);
        }
        let written = tests_folder(build);
        if first_look {
            match std::fs::remove_dir_all(&written) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    return Err(format!("cannot clear {}: {e}", written.display()));
                }
                _ => {}
            }
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

    // The reach of a Codex or goose reviewer that only reads, with an empty folder of its own.
    fn begin_reading(
        &self,
        model: &RoleModel,
        worktree: &Path,
        build: &Path,
    ) -> Result<Option<Reach>, String> {
        // parity: harness.reviewer-reads-repo
        if !matches!(model.harness.harness(), Harness::Codex | Harness::Goose) {
            return Ok(None);
        }
        let folder = build.join(READING_FOLDER);
        // Empty, whatever an earlier look left: the reviewer reads this as its own.
        match std::fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("cannot clear {}: {e}", folder.display()));
            }
            _ => {}
        }
        std::fs::create_dir_all(&folder)
            .map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
        let code = [worktree.to_owned()];
        Ok(Some(self.fenced_reach(
            &folder,
            &code,
            Vec::new(),
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
                    Ok(true) => finding.test = Some(test),
                    // A pipe in the why can read as a path. With no file left, it
                    // is part of the why again.
                    Ok(false) => {
                        self.notes.push(refused(issue, &finding, &test, NO_FILE));
                        finding.why = format!("{}|{test}", finding.why);
                    }
                    Err(reason) => self.notes.push(refused(issue, &finding, &test, &reason)),
                }
            }
            checked.push(finding);
        }
        checked
    }
}

const NO_FILE: &str = "the reviewer left no such file";

// The note for a test that is dropped while its finding stays.
fn refused(issue: u64, finding: &Finding, test: &str, reason: &str) -> String {
    format!(
        "issue #{issue}: the test {test} for {}:{} is dropped and the finding stays: {reason}",
        finding.file, finding.line
    )
}

// Checks `test` below `written` and copies it below `kept`: whether the
// reviewer left the file, and why a file it left is refused.
fn keep(written: &Path, kept: &Path, worktree: &Path, test: &str) -> Result<bool, String> {
    let rel = Path::new(test);
    let Some(text) = worker_folder::read(written, rel).map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    // A link there is as much the worktree's as a file.
    match worktree.join(rel).symlink_metadata() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => return Err("the worktree already holds a file of that name".to_owned()),
        Err(e) => return Err(format!("cannot look for it in the worktree: {e}")),
    }
    worker_folder::write(kept, rel, &text).map_err(|e| format!("cannot keep a copy: {e}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AgentHarness, ContextSize, Effort, EndpointUrl, ModelHost, ModelServer};
    use crate::test::Rig;

    fn on(harness: AgentHarness) -> RoleModel {
        RoleModel {
            model: "m".to_owned().try_into().unwrap(),
            effort: Effort::Low,
            harness,
            tools: None,
        }
    }

    fn pi() -> AgentHarness {
        AgentHarness::Pi(ModelServer {
            host: ModelHost::Url(EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap()),
            context: ContextSize::try_from(65_536).unwrap(),
        })
    }

    fn goose() -> AgentHarness {
        AgentHarness::Goose(ModelServer {
            host: ModelHost::Url(EndpointUrl::try_from("http://box:11434/v1".to_owned()).unwrap()),
            context: ContextSize::try_from(65_536).unwrap(),
        })
    }

    #[test]
    fn a_reviewer_is_asked_for_tests_on_claude_code_pi_and_goose() {
        let harnesses = || [AgentHarness::ClaudeCode, pi(), AgentHarness::Codex, goose()];

        let plain = Rig::new("shep");
        let runner = plain.open().unwrap();
        let runner = runner.lock().unwrap();
        for harness in harnesses() {
            assert_eq!(
                runner.test_command_for(&on(harness.clone())),
                None,
                "{harness:?}"
            );
        }

        let rig = Rig::new("shep");
        rig.test_command("cargo test --test {name}");
        let runner = rig.open().unwrap();
        let runner = runner.lock().unwrap();
        for harness in harnesses() {
            let asked = runner.test_command_for(&on(harness.clone()));
            assert_eq!(
                asked.map(TestCommand::as_str),
                Some("cargo test --test {name}"),
                "{harness:?}"
            );
        }
    }
}
