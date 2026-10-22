//! Every process kelpie starts, with the secrets kelpie reads kept out of it
//!
//! A gateway's key is a variable in kelpie's own environment, which every
//! child would inherit. The crate cannot unset a variable of its own, so each
//! process it starts is built here, with every hidden variable removed.

use std::collections::BTreeSet;
use std::ffi::OsStr;
#[cfg(any(windows, test))]
use std::ffi::OsString;
#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
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

/// The POSIX shell that runs a command string, as `sh -c`
///
/// On Windows it is Git for Windows' `sh.exe`, which kelpie requires and finds
/// itself: a bare `sh` on the `PATH` could be any program. Where Git for Windows
/// is not installed it is `sh.exe`, which fails to start with "not found".
pub fn sh() -> PathBuf {
    posix_tool("/bin/sh", "sh")
}

/// The POSIX `env` that puts variables in front of a program
///
/// Found as [`sh`] is.
pub fn env() -> PathBuf {
    posix_tool("/usr/bin/env", "env")
}

#[cfg(unix)]
fn posix_tool(path: &str, _name: &str) -> PathBuf {
    PathBuf::from(path)
}

#[cfg(windows)]
fn posix_tool(_path: &str, name: &str) -> PathBuf {
    git_for_windows_tool(name, &|var| std::env::var_os(var), &Path::is_file)
        .unwrap_or_else(|| PathBuf::from(format!("{name}.exe")))
}

// `name.exe` from the `usr/bin` of a Git for Windows install: one beside the
// `git.exe` on the `PATH`, else one at a default place.
#[cfg(any(windows, test))]
fn git_for_windows_tool(
    name: &str,
    var: &dyn Fn(&str) -> Option<OsString>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let on_path = var("PATH")
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let beside_git = on_path
        .split(';')
        .map(Path::new)
        .filter(|dir| exists(&dir.join("git.exe")))
        .filter_map(Path::parent)
        .map(Path::to_path_buf);
    let default = [
        ("ProgramFiles", "Git"),
        ("ProgramW6432", "Git"),
        ("ProgramFiles(x86)", "Git"),
        ("LOCALAPPDATA", "Programs/Git"),
    ]
    .into_iter()
    .filter_map(|(base, install)| var(base).map(|base| PathBuf::from(base).join(install)));
    beside_git
        .chain(default)
        .map(|root| root.join("usr").join("bin").join(format!("{name}.exe")))
        .find(|candidate| exists(candidate))
}

/// `s` as one word to a POSIX shell
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shep_kelpie_test_support::{linked_worktree, write_script};
    use std::ffi::OsString;

    // An environment of `vars`, and a disk of `files`.
    fn machine<'a>(
        vars: &'a [(&'a str, &'a str)],
        files: &'a [&'a str],
    ) -> (
        impl Fn(&str) -> Option<OsString> + 'a,
        impl Fn(&Path) -> bool + 'a,
    ) {
        (
            move |name: &str| {
                vars.iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| OsString::from(v))
            },
            move |path: &Path| files.iter().any(|f| Path::new(f) == path),
        )
    }

    #[test]
    fn git_for_windows_is_found_at_its_default_install() {
        let (var, exists) = machine(
            &[("ProgramFiles", "C:/Program Files")],
            &["C:/Program Files/Git/usr/bin/sh.exe"],
        );
        assert_eq!(
            git_for_windows_tool("sh", &var, &exists),
            Some(PathBuf::from("C:/Program Files/Git/usr/bin/sh.exe"))
        );
    }

    #[test]
    fn git_for_windows_is_found_beside_the_git_on_the_path() {
        let (var, exists) = machine(
            &[("PATH", "D:/bin;D:/tools/Git/cmd")],
            &["D:/tools/Git/cmd/git.exe", "D:/tools/Git/usr/bin/env.exe"],
        );
        assert_eq!(
            git_for_windows_tool("env", &var, &exists),
            Some(PathBuf::from("D:/tools/Git/usr/bin/env.exe"))
        );
    }

    #[test]
    fn no_git_for_windows_is_none() {
        let (var, exists) = machine(&[("ProgramFiles", "C:/Program Files")], &[]);
        assert_eq!(git_for_windows_tool("sh", &var, &exists), None);
    }

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
