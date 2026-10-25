//! The workflows `origin/main` holds

use std::path::Path;

use super::{WorktreeError, git};
use crate::workflows::{is_workflow_file, runs_on_a_pull_request};

const FOLDER: &str = ".github/workflows";

/// Whether any workflow `origin/main` holds runs on a pull request, as the
/// base was last fetched
///
/// Reads the base and not the branch, so a branch cannot switch CI off by
/// deleting a workflow. A base with no workflows folder has none.
///
/// # Errors
///
/// [`WorktreeError`] naming the git command that failed.
pub fn runs_on_pull_requests(repo: &Path, base: &str) -> Result<bool, WorktreeError> {
    let tip = format!("origin/{base}");
    let listed = git(
        repo,
        ["ls-tree", "--name-only", &tip, "--", &format!("{FOLDER}/")],
    )?;
    for path in listed.lines().filter(|path| is_workflow_file(path)) {
        let text = git(repo, ["show", &format!("{tip}:{path}")])?;
        if runs_on_a_pull_request(&text) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUSH_MAIN: &str = "on:\n  push:\n    branches: [main]\njobs:\n  t:\n";
    const PULL_REQUEST: &str = "on: pull_request\njobs:\n  t:\n";

    // A repo whose `origin/main` holds `files`
    fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), ["init", "--quiet", "--initial-branch=main"]).unwrap();
        std::fs::write(dir.path().join("README.md"), "a project\n").unwrap();
        for (name, text) in files {
            let at = dir.path().join(FOLDER).join(name);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        git(dir.path(), ["add", "."]).unwrap();
        let commit = ["-c", "user.name=t", "-c", "user.email=t@example.invalid"];
        git(
            dir.path(),
            [commit.as_slice(), &["commit", "--quiet", "-m", "first"]].concat(),
        )
        .unwrap();
        git(
            dir.path(),
            ["update-ref", "refs/remotes/origin/main", "HEAD"],
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_base_runs_on_pull_requests_when_one_workflow_does() {
        let dir = repo_with(&[("release.yml", PUSH_MAIN), ("ci.yaml", PULL_REQUEST)]);
        assert!(runs_on_pull_requests(dir.path(), "main").unwrap());
    }

    #[test]
    fn a_base_whose_workflows_run_on_main_alone_does_not() {
        let dir = repo_with(&[("release.yml", PUSH_MAIN), ("notes.md", PULL_REQUEST)]);
        assert!(!runs_on_pull_requests(dir.path(), "main").unwrap());
    }

    #[test]
    fn a_base_with_no_workflows_folder_does_not() {
        let dir = repo_with(&[]);
        assert!(!runs_on_pull_requests(dir.path(), "main").unwrap());
    }

    #[test]
    fn a_base_that_is_not_fetched_is_an_error() {
        let dir = repo_with(&[("ci.yml", PULL_REQUEST)]);
        assert!(runs_on_pull_requests(dir.path(), "trunk").is_err());
    }
}
