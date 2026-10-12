//! A marketplace's repo cloned and fetched with the system's `git`

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::Stdio;

use crate::ports::{Git, GitError};

// Never a prompt for a password, and never a transport that runs a program.
const PROTOCOLS: &str = "file:https:ssh:git";

/// Syncs a repo with the system's `git`, one shallow copy of one ref
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemGit;

impl Git for SystemGit {
    fn sync(&self, url: &str, reference: Option<&str>, into: &Path) -> Result<(), GitError> {
        if into.join(".git").exists() {
            let target = reference.unwrap_or("HEAD");
            run(
                into,
                ["fetch", "--quiet", "--depth", "1", "origin", "--", target],
            )?;
            return run(into, ["reset", "--quiet", "--hard", "FETCH_HEAD"]);
        }
        // A clone that fails halfway leaves nothing at `into`, which a later
        // fetch would take for a copy.
        let parent = into
            .parent()
            .ok_or_else(|| GitError(format!("{} has no parent folder", into.display())))?;
        fs::create_dir_all(parent).map_err(|e| GitError(format!("cannot create folder: {e}")))?;
        let name = into.file_name().unwrap_or_default().to_string_lossy();
        let partial = parent.join(format!(".{name}.cloning"));
        let _ = fs::remove_dir_all(&partial);
        let mut git = crate::spawn::command("git");
        git.current_dir(parent)
            .args(["clone", "--quiet", "--depth", "1"]);
        if let Some(reference) = reference {
            git.args(["--branch", reference]);
        }
        git.args(["--", url]).arg(&partial);
        let cloned = finish(git);
        let moved = cloned.and_then(|()| {
            fs::rename(&partial, into).map_err(|e| GitError(format!("cannot move the clone: {e}")))
        });
        if moved.is_err() {
            let _ = fs::remove_dir_all(&partial);
        }
        moved
    }
}

fn run<I, S>(repo: &Path, args: I) -> Result<(), GitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut git = crate::worktree::in_repo(repo);
    git.args(args);
    finish(git)
}

fn finish(mut git: std::process::Command) -> Result<(), GitError> {
    let output = git
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", PROTOCOLS)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| GitError(format!("cannot run git: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(GitError(format!("git failed: {}", stderr.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::git;

    // A repo with one commit holding `file`, and its `file://` address
    fn repo_with(dir: &Path, file: &str) -> String {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "--quiet", "--initial-branch=main"]);
        commit(dir, file);
        format!("file://{}", dir.display())
    }

    fn commit(dir: &Path, file: &str) {
        fs::write(dir.join(file), "x\n").unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "--quiet", "-m", file]);
    }

    #[test]
    fn it_clones_a_repo_and_then_fetches_what_it_gained() {
        let home = tempfile::tempdir().unwrap();
        let url = repo_with(&home.path().join("remote"), "one");
        let into = home.path().join("marketplaces/house");

        SystemGit.sync(&url, None, &into).unwrap();
        assert!(into.join("one").is_file());
        assert!(!home.path().join("marketplaces/.house.cloning").exists());

        commit(&home.path().join("remote"), "two");
        SystemGit.sync(&url, None, &into).unwrap();
        assert!(into.join("two").is_file());
    }

    #[test]
    fn it_clones_the_ref_it_is_given() {
        let home = tempfile::tempdir().unwrap();
        let remote = home.path().join("remote");
        let url = repo_with(&remote, "one");
        git(&remote, &["tag", "v1"]);
        commit(&remote, "two");
        let into = home.path().join("marketplaces/house");

        SystemGit.sync(&url, Some("v1"), &into).unwrap();
        assert!(into.join("one").is_file());
        assert!(!into.join("two").exists());
    }

    #[test]
    fn a_failed_clone_leaves_nothing_and_a_failed_fetch_leaves_the_copy() {
        let home = tempfile::tempdir().unwrap();
        let remote = home.path().join("remote");
        let url = repo_with(&remote, "one");
        let into = home.path().join("marketplaces/house");

        let gone = format!("file://{}", home.path().join("none").display());
        assert!(SystemGit.sync(&gone, None, &into).is_err());
        assert!(!into.exists());
        assert!(!home.path().join("marketplaces/.house.cloning").exists());

        SystemGit.sync(&url, None, &into).unwrap();
        fs::remove_dir_all(&remote).unwrap();
        assert!(SystemGit.sync(&url, None, &into).is_err());
        assert!(into.join("one").is_file());
    }

    #[test]
    fn a_program_transport_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let into = home.path().join("marketplaces/house");
        let marker = home.path().join("ran");
        let url = format!("ext::sh -c touch% {}", marker.display());
        assert!(SystemGit.sync(&url, None, &into).is_err());
        assert!(!marker.exists());
    }
}
