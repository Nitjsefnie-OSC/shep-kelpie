//! A reviewer's test, run before the fix and after it
//!
//! With `ci.test_command` set, a finding may carry a failing test the reviewer
//! wrote, kept in `<worker>/review-tests-<issue>` ([`super::attached`]). Once
//! the head a round read is known, each such test is put in the worktree at
//! its path, run through the [`TestRunner`](crate::ports::TestRunner), and
//! taken out again, so the worktree is as clean as it was. When the fix has
//! moved the head and the worktree holds it, the same run is made again.
//!
//! The runs are recorded in the work item's `test_checks` and reported, and
//! they block nothing: whatever the outcomes, the pass goes on as it did.

use std::io;
use std::path::{Component, Path, PathBuf};

use super::super::Runner;
use super::super::turn::start_of;
use super::attached::kept_folder;
use crate::ports::{CEILING, Finding, TestOutcome, TestRun};
use crate::state::StateError;
use crate::work_item::TestCheck;
use crate::worker_folder;

/// What a check says when the worktree is not at the head the reviewer read
const NOT_AT_THE_HEAD: &str = "the worktree is not at the head the reviewer read";

// What the worker folder holds while a test is in the worktree: a kelpie
// stopped mid-run leaves the file there, and this says so.
fn placed_name(issue: u64) -> String {
    format!("test-placed-{issue}")
}

impl Runner {
    /// The runs of `found`'s tests at `head`, the head the round read, one
    /// check each, with no run after the fix yet
    ///
    /// Nothing is run when the worktree is somewhere else: the outcome there
    /// would say nothing of what the reviewer read.
    pub(super) fn tests_before(&self, round: u32, head: &str, found: &[Finding]) -> Vec<TestCheck> {
        self.clear_stranded();
        let tested: Vec<(&Finding, &str)> = found
            .iter()
            .filter_map(|f| Some((f, f.test.as_deref()?)))
            .collect();
        if tested.is_empty() {
            return Vec::new();
        }
        let own = self.worktree_head().as_deref() == Some(head);
        tested
            .into_iter()
            .map(|(finding, test)| TestCheck {
                file: finding.file.clone(),
                line: finding.line,
                what: finding.what.clone(),
                round,
                before: match own {
                    true => self.run_kept(test),
                    false => TestOutcome::NotRun {
                        reason: NOT_AT_THE_HEAD.to_owned(),
                    },
                },
                after: None,
            })
            .collect()
    }

    /// Runs the tests of `found`, the findings a fix was sent in `round`,
    /// at the worktree's head, and records each as the check's `after`
    ///
    /// Returns the round's checks as they now stand.
    ///
    /// # Errors
    ///
    /// [`StateError`] when the state cannot be saved.
    pub(super) fn tests_after(
        &mut self,
        round: u32,
        found: &[Finding],
    ) -> Result<Vec<TestCheck>, StateError> {
        self.clear_stranded();
        let mut ran = Vec::new();
        if let Some(item) = self.current() {
            for finding in found {
                let Some(test) = finding.test.as_deref() else {
                    continue;
                };
                if let Some(at) = waiting(&item.test_checks, round, finding) {
                    ran.push((at, self.run_kept(test)));
                }
            }
        }
        if !ran.is_empty() {
            self.update(|item| {
                for (at, outcome) in ran {
                    item.test_checks[at].after = Some(outcome);
                }
            })?;
        }
        Ok(self.round_checks(round, found))
    }

    /// The checks `found`'s tests have in `round`, as recorded
    pub(super) fn round_checks(&self, round: u32, found: &[Finding]) -> Vec<TestCheck> {
        let Some(item) = self.current() else {
            return Vec::new();
        };
        found
            .iter()
            .filter(|f| f.test.is_some())
            .filter_map(|f| latest(&item.test_checks, round, f))
            .map(|at| item.test_checks[at].clone())
            .collect()
    }

    // Takes out the test a stopped kelpie left in the worktree, and says so: a
    // stray file is one the worker may commit and the next clean-worktree
    // check would rule on.
    fn clear_stranded(&self) {
        let Some(item) = self.current() else {
            return;
        };
        let (issue, name) = (item.issue, placed_name(item.issue));
        let text = match worker_folder::read(&self.paths.worker, Path::new(&name)) {
            Ok(Some(text)) => text,
            Ok(None) => return,
            Err(e) => {
                self.notes.push(format!(
                    "issue #{issue}: cannot read {name}, so a test a stopped run left in the worktree is not looked for: {e}"
                ));
                return;
            }
        };
        let mut paths = text.lines().map(PathBuf::from);
        let file = paths.next().filter(|file| inside(file));
        let kept = kept_folder(&self.paths.worker, issue);
        // Only the reviewer's own text is taken out: a file that reads
        // otherwise is not what kelpie put there.
        let ours =
            file.as_deref()
                .is_some_and(|file| match worker_folder::read(&item.worktree, file) {
                    Ok(None) => true,
                    Ok(Some(held)) => {
                        matches!(worker_folder::read(&kept, file), Ok(Some(copy)) if copy == held)
                    }
                    Err(_) => false,
                });
        let record = self.paths.worker.join(&name);
        let left = match file {
            Some(file) if ours => Placed {
                root: item.worktree.clone(),
                file: Some(file),
                made: paths.filter(|dir| inside(dir)).collect(),
                record: Some(record),
            }
            .take_out(),
            Some(file) => {
                let mut left = vec![format!("{}: it is not the reviewer's text", file.display())];
                left.extend(
                    std::fs::remove_file(&record)
                        .err()
                        .map(|e| format!("{name}: {e}")),
                );
                left
            }
            None => vec![format!("{name}: it names no path inside the worktree")],
        };
        self.notes.push(match left.is_empty() {
            true => format!(
                "issue #{issue}: kelpie was stopped during a test run, and took the test out of the worktree"
            ),
            false => format!(
                "issue #{issue}: kelpie was stopped during a test run, and the worktree may be dirty, remove by hand: {}",
                left.join("; ")
            ),
        });
    }

    // The kept copy of `test` run in the worktree, which is left as it was.
    fn run_kept(&self, test: &str) -> TestOutcome {
        match self.try_run_kept(test) {
            Ok(outcome) => outcome,
            Err(reason) => TestOutcome::NotRun { reason },
        }
    }

    fn try_run_kept(&self, test: &str) -> Result<TestOutcome, String> {
        let item = self.current().ok_or("the work item is gone")?;
        let template = (self.settings.ci.test_command.as_ref())
            .ok_or("the project has no `ci.test_command` now")?;
        let command = fill(template.as_str(), test)?;
        let rel = Path::new(test);
        let kept = kept_folder(&self.paths.worker, item.issue);
        let text = worker_folder::read(&kept, rel)
            .map_err(|e| format!("cannot read the kept copy of {test}: {e}"))?
            .ok_or_else(|| format!("the kept copy of {test} is gone"))?;
        // The worker's own fence, never the maintainer's reach: the file and
        // the code under it are a model's.
        let reach = self.worker_reach(item, start_of(item), None)?;
        let record = self.paths.worker.join(placed_name(item.issue));
        let placed = Placed::put(&item.worktree, rel, &text, &record)?;
        let outcome = self.ports.test_runner.run(&TestRun {
            command,
            cwd: item.worktree.clone(),
            reach,
            settings: (self.paths.worker).join(format!("test-run-{}.sandbox.json", item.issue)),
            door: self.paths.door.clone(),
            ceiling: CEILING,
        });
        // The outcome stands, but a worktree the run left dirty is the
        // maintainer's to know: the next run of this path refuses to start
        // over a file that is still there.
        let left = placed.take_out();
        if !left.is_empty() {
            self.notes.push(format!(
                "issue #{}: the test {test} left the worktree dirty, remove by hand: {}",
                item.issue,
                left.join("; ")
            ));
        }
        Ok(outcome)
    }
}

// Whether `path` is a path of one or more plain steps, so it stays below the
// folder it is joined to.
fn inside(path: &Path) -> bool {
    path.components().next().is_some()
        && (path.components()).all(|step| matches!(step, Component::Normal(_)))
}

// `template` with the test's path at `{test}` and its file stem at `{name}`.
// The command goes to a shell, so a path it could read as anything but a
// path is refused.
fn fill(template: &str, test: &str) -> Result<String, String> {
    // The path is joined to the worktree, so it must stay below it, whatever
    // the state file holds.
    if !inside(Path::new(test)) {
        return Err(format!(
            "the path {test} is not a relative path inside the worktree, so it is not run"
        ));
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/');
    if !test.chars().all(plain) || test.starts_with('-') {
        return Err(format!(
            "the path {test} holds characters a shell would read, so no command is built from it"
        ));
    }
    let name = Path::new(test)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("the path {test} has no file name"))?;
    let command = template.replace("{test}", test).replace("{name}", name);
    // An empty command is a shell that exits 0 having run nothing.
    match command.trim().is_empty() {
        true => Err("`ci.test_command` is empty, so there is nothing to run".to_owned()),
        false => Ok(command),
    }
}

// The newest check of `finding` in `round`, if any.
fn latest(checks: &[TestCheck], round: u32, finding: &Finding) -> Option<usize> {
    checks.iter().rposition(|c| c.is_for(round, finding))
}

// The newest check of `finding` in `round` that no run after the fix has
// been made for.
fn waiting(checks: &[TestCheck], round: u32, finding: &Finding) -> Option<usize> {
    latest(checks, round, finding).filter(|at| checks[*at].after.is_none())
}

// A test file put in the worktree, and the folders made for it
//
// Until it is taken out, a record of it sits in the worker folder, so a kelpie
// that is stopped in the middle of a run can take it out when it starts again.
struct Placed {
    root: PathBuf,
    // none when the worktree held the file already: it is the worker's
    file: Option<PathBuf>,
    // below `root`, top down
    made: Vec<PathBuf>,
    record: Option<PathBuf>,
}

impl Placed {
    // `text` at `rel` below `root`, with `record` written first.
    //
    // A file already there with this very text is the worker's own copy of the
    // reviewer's test, committed with its fix: it is run as it is, and left.
    // Any other file there is not the reviewer's test, and nothing is put over it.
    fn put(root: &Path, rel: &Path, text: &str, record: &Path) -> Result<Self, String> {
        match root.join(rel).symlink_metadata() {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                return match worker_folder::read(root, rel) {
                    Ok(Some(held)) if held == text => Ok(Self {
                        root: root.to_owned(),
                        file: None,
                        made: Vec::new(),
                        record: None,
                    }),
                    _ => Err("the worktree already holds another file of that name".to_owned()),
                };
            }
            Err(e) => return Err(format!("cannot look for it in the worktree: {e}")),
        }
        let mut made = Vec::new();
        let mut dir = PathBuf::new();
        for folder in rel.parent().into_iter().flat_map(Path::components) {
            dir.push(folder);
            if matches!(root.join(&dir).symlink_metadata(), Err(e) if e.kind() == io::ErrorKind::NotFound)
            {
                made.push(dir.clone());
            }
        }
        let placed = Self {
            root: root.to_owned(),
            file: Some(rel.to_owned()),
            made,
            record: Some(record.to_owned()),
        };
        if let Err(e) = placed.write_record() {
            return Err(format!(
                "cannot record the run, so the test is not put in the worktree: {e}"
            ));
        }
        match worker_folder::write(root, rel, text) {
            Ok(()) => Ok(placed),
            Err(e) => {
                let left = placed.take_out();
                let left = match left.is_empty() {
                    true => String::new(),
                    false => format!(" and cannot clear it: {}", left.join("; ")),
                };
                Err(format!("cannot put the test in the worktree: {e}{left}"))
            }
        }
    }

    // The file's path, then the path of each folder made, a line each.
    fn write_record(&self) -> Result<(), String> {
        let (Some(record), Some(file)) = (&self.record, &self.file) else {
            return Ok(());
        };
        let (Some(folder), Some(name)) = (record.parent(), record.file_name()) else {
            return Err("the record has no place".to_owned());
        };
        let paths = std::iter::once(file).chain(&self.made);
        let text = paths
            .map(|p| format!("{}\n", p.display()))
            .collect::<String>();
        worker_folder::write(folder, Path::new(name), &text).map_err(|e| e.to_string())
    }

    // Removes the file, then the folders made for it, then the record, and
    // names each that stays: a folder the run put something in is still the
    // worktree's mess.
    fn take_out(self) -> Vec<String> {
        let mut left = Vec::new();
        let mut gone = |shown: &Path, result: io::Result<()>| match result {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                left.push(format!("{}: {e}", shown.display()));
            }
            _ => {}
        };
        if let Some(rel) = &self.file {
            let file = self.root.join(rel);
            gone(rel, std::fs::remove_file(&file));
        }
        for rel in self.made.iter().rev() {
            let dir = self.root.join(rel);
            gone(rel, std::fs::remove_dir(&dir));
        }
        if let Some(record) = &self.record {
            gone(record, std::fs::remove_file(record));
        }
        left
    }
}

#[cfg(test)]
mod tests;
