//! The runner's own git, cloning a plugin marketplace into kelpie's home

use std::path::Path;
use std::process::{Command, Stdio};

use crate::ports::Marketplaces;

/// Clones with the `git` on `PATH`, shallow, and never asks for a password
#[derive(Debug, Clone, Copy, Default)]
pub struct GitMarketplaces;

impl Marketplaces for GitMarketplaces {
    fn fetch(&self, url: &str, into: &Path) -> Result<(), String> {
        if url.starts_with('-') {
            return Err(format!("{url:?} is not a git URL"));
        }
        if into.join(".git").is_dir() {
            git(into, &["remote", "set-url", "origin", url])?;
            git(
                into,
                &["fetch", "--quiet", "--depth", "1", "origin", "HEAD"],
            )?;
            return git(into, &["reset", "--quiet", "--hard", "FETCH_HEAD"]);
        }
        if let Some(parent) = into.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot make {}: {}", parent.display(), e.kind()))?;
        }
        // A folder a clone started and did not finish is cloned over.
        match std::fs::remove_dir_all(into) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("cannot clear {}: {}", into.display(), e.kind()));
            }
            _ => {}
        }
        let into = into.to_string_lossy();
        run(base()
            .args(["clone", "--quiet", "--depth", "1", "--", url])
            .arg(&*into))
    }
}

// A git that never prompts, and never starts a program a URL names.
fn base() -> Command {
    let mut git = crate::spawn::command("git");
    git.args(["-c", "protocol.ext.allow=never"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    git
}

fn git(in_folder: &Path, args: &[&str]) -> Result<(), String> {
    run(base().arg("-C").arg(in_folder).args(args))
}

fn run(git: &mut Command) -> Result<(), String> {
    let output = git.output().map_err(|e| format!("cannot run git: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("git {}: {}", output.status, stderr.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{git as rig_git, write_in};

    fn origin(home: &Path) -> std::path::PathBuf {
        let origin = home.join("market");
        std::fs::create_dir_all(&origin).unwrap();
        rig_git(&origin, &["init", "--quiet", "-b", "main"]);
        write_in(&origin, "one.txt", "one\n");
        rig_git(&origin, &["add", "."]);
        rig_git(&origin, &["commit", "--quiet", "-m", "one"]);
        origin
    }

    #[test]
    fn a_marketplace_is_cloned_and_then_brought_up_to_date() {
        let home = tempfile::tempdir().unwrap();
        let origin = origin(home.path());
        let url = origin.to_str().unwrap();
        let into = home.path().join("marketplaces/house");
        GitMarketplaces.fetch(url, &into).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("one.txt")).unwrap(),
            "one\n"
        );
        write_in(&origin, "two.txt", "two\n");
        rig_git(&origin, &["add", "."]);
        rig_git(&origin, &["commit", "--quiet", "-m", "two"]);
        GitMarketplaces.fetch(url, &into).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("two.txt")).unwrap(),
            "two\n"
        );
    }

    #[test]
    fn a_fetch_that_fails_leaves_what_was_cloned() {
        let home = tempfile::tempdir().unwrap();
        let origin = origin(home.path());
        let into = home.path().join("marketplaces/house");
        GitMarketplaces
            .fetch(origin.to_str().unwrap(), &into)
            .unwrap();
        let gone = home.path().join("gone");
        assert!(
            GitMarketplaces
                .fetch(gone.to_str().unwrap(), &into)
                .is_err()
        );
        assert!(into.join("one.txt").is_file());
    }
}
