//! Every process kelpie starts, with the secrets kelpie reads kept out of it
//!
//! A gateway's key is a variable in kelpie's own environment, which every
//! child would inherit. The crate cannot unset a variable of its own, so each
//! process it starts is built here, with every hidden variable removed.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::process::Command;
use std::sync::{Mutex, PoisonError};

// Only ever added to: a variable once hidden stays hidden for the process's life.
static HIDDEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Keeps `vars` out of every process started from now on
pub fn hide(vars: impl IntoIterator<Item = String>) {
    let mut hidden = HIDDEN.lock().unwrap_or_else(PoisonError::into_inner);
    hidden.extend(vars);
}

/// `program`, as [`Command::new`] makes it, with every hidden variable unset
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    let hidden = HIDDEN.lock().unwrap_or_else(PoisonError::into_inner);
    for var in hidden.iter() {
        command.env_remove(var);
    }
    command
}

/// `s` as one word to a POSIX shell
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shep_kelpie_test_support::{linked_worktree, write_script};

    #[test]
    fn a_word_is_one_word_to_a_posix_shell() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    // Cargo sets it for every test binary it runs, and nothing kelpie starts reads it.
    const SET: &str = "CARGO_PKG_DESCRIPTION";

    #[test]
    fn a_command_reviewer_and_a_git_call_never_see_a_hidden_variable() {
        assert!(std::env::var_os(SET).is_some(), "{SET} is set under cargo");
        hide([SET.to_owned()]);
        let dir = tempfile::tempdir().unwrap();
        let (repo, _worktree) = linked_worktree(dir.path());
        let shown = crate::worktree::in_repo(&repo)
            .args(["-c", "alias.seen=!env", "seen"])
            .output()
            .unwrap();
        let shown = String::from_utf8_lossy(&shown.stdout).into_owned();
        assert!(shown.contains("PATH="), "{shown}");
        assert!(!shown.contains(SET), "git saw it");

        let script = dir.path().join("review");
        let out = dir.path().join("out");
        write_script(
            &script,
            "#!/bin/sh\nmkdir -p \"$QWEN_REVIEW_OUT\"\n\
             env > \"$QWEN_REVIEW_OUT/env.txt\"\n",
        );
        command(&script)
            .env("QWEN_REVIEW_OUT", &out)
            .status()
            .unwrap();
        let env = std::fs::read_to_string(out.join("env.txt")).unwrap();
        assert!(env.contains("QWEN_REVIEW_OUT="), "{env}");
        assert!(!env.contains(SET), "the reviewer's script saw it");
    }
}
