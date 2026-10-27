//! A Codex reviewer and retro: a shell that reads, and no patch

use std::path::{Path, PathBuf};

use super::hook_tests::codex_hooks;
use super::review_tests::reviewer;
use super::tests::{WORKER_ID, World, id, strings};
use super::*;
use crate::ports::Role;

// The command of the hook that holds `matcher`'s tool, among the call's.
fn hook_for(argv: &[String], matcher: &str) -> Option<String> {
    let hooks = codex_hooks(argv, "PreToolUse")?;
    let entry = hooks
        .as_array()?
        .iter()
        .find(|h| h["matcher"].as_str() == Some(matcher))?
        .clone();
    Some(entry["hooks"][0]["command"].as_str()?.to_owned())
}

fn shell_is_off(argv: &[String]) -> bool {
    argv.windows(2).any(|a| a == ["--disable", "shell_tool"])
}

// The hook refuses as Codex reads a refusal: exit 2, the reason on stderr.
#[track_caller]
fn refuses_every_patch(command: &str) {
    let ran = std::process::Command::new(crate::spawn::sh())
        .args(["-c", command])
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(2));
    assert_eq!(ran.stderr, b"kelpie: this call writes no files\n");
}

#[test]
fn a_codex_reviewer_in_a_fence_has_a_read_only_shell_and_no_patch() {
    let w = World::new();
    let folder = w.path("build/review-read");
    let mut call = reviewer(&w, Some(&folder));
    call.reach.fence.as_mut().unwrap().write.clear();

    let argv = strings(&call, None, None);

    assert!(!shell_is_off(&argv));
    let shell = hook_for(&argv, "^Bash$").expect("a hook judges the shell");
    assert!(shell.contains("'guard'"), "{shell}");
    assert!(shell.contains("'--read-only'"), "{shell}");
    assert!(!shell.contains("--pin-folder"), "{shell}");
    refuses_every_patch(&hook_for(&argv, "^apply_patch$").expect("a hook holds apply_patch"));
}

#[test]
fn a_codex_reviewer_asked_for_tests_keeps_its_patch_hook_for_the_tests_folder() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let call = reviewer(&w, Some(&folder));

    let argv = strings(&call, None, None);

    assert!(!shell_is_off(&argv));
    let folder_arg = format!("'{}'", folder.display());
    assert_eq!(
        hook_for(&argv, "^apply_patch$").unwrap(),
        format!("'/k/kelpie' 'confine' {folder_arg} {folder_arg}")
    );
    let shell = hook_for(&argv, "^Bash$").unwrap();
    assert!(shell.contains("'--read-only'"), "{shell}");

    let policy = policy(
        &call,
        &w.path("login"),
        &Files::of(&call),
        Path::new("/nowhere/codex"),
        &[],
    );
    assert!(policy.write.contains(&folder), "{:?}", policy.write);
    let worktree = w.path("wt");
    let in_worktree: Vec<&PathBuf> = (policy.write.iter())
        .filter(|p| p.starts_with(&worktree) || worktree.starts_with(p))
        .collect();
    assert!(in_worktree.is_empty(), "{in_worktree:?}");
}

#[test]
fn a_codex_call_reads_the_dependency_sources_it_is_given_and_no_credential_of_them() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    // The folders `crate::dep_sources` finds, as the runner puts them in a call's reach.
    let sources = [
        w.path("cargo/registry/src/serde-1.0.2"),
        w.path("cargo/git/checkouts/shep-core-1a2b3c"),
        w.path("rustup/toolchains/stable/lib/rustlib/src"),
        w.path("rustup/toolchains/stable/share/doc"),
    ];
    for folder in &sources {
        call.reach.read.push(folder.clone());
        call.reach.fence.as_mut().unwrap().read.push(folder.clone());
    }
    let policy = policy(
        &call,
        &w.path("login"),
        &Files::of(&call),
        Path::new("/nowhere/codex"),
        &[],
    );
    for folder in &sources {
        assert!(policy.read.contains(folder), "{}", folder.display());
        assert!(
            !policy
                .write
                .iter()
                .any(|writable| folder.starts_with(writable)),
            "{} is writable",
            folder.display()
        );
    }
    for secret in ["~/.cargo/credentials", "~/.npmrc", "~/.netrc", "~/.ssh/**"] {
        assert!(policy.no_read.iter().any(|r| r == secret), "{secret}");
    }
}

#[test]
fn a_retro_has_a_read_only_shell_and_no_write_in_the_sandbox() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::Resume(id(WORKER_ID)));
    call.tools = Tools::Retro;
    assert!(
        !call.reach.fence.as_ref().unwrap().write.is_empty(),
        "the fence it runs under can write"
    );

    let argv = strings(&call, None, None);

    assert!(!shell_is_off(&argv), "{argv:?}");
    let shell = hook_for(&argv, "^Bash$").expect("a hook judges the shell");
    assert!(shell.contains("'--read-only'"), "{shell}");
    refuses_every_patch(&hook_for(&argv, "^apply_patch$").unwrap());

    let files = Files::of(&call);
    let policy = policy(
        &call,
        &w.path("login"),
        &files,
        Path::new("/nowhere/codex"),
        &[],
    );
    let mut own: Vec<_> = WRITTEN.map(|name| files.home.join(name)).into();
    own.extend([files.home.join(INSTALLATION_ID), files.scratch.clone()]);
    let mut written = policy.write.clone();
    written.sort();
    own.sort();
    assert_eq!(written, own);
}

#[test]
fn a_worker_keeps_the_workers_guard_and_its_write_folders() {
    let w = World::new();
    let call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));

    let argv = strings(&call, None, None);

    assert!(!shell_is_off(&argv));
    let shell = hook_for(&argv, "^Bash$").unwrap();
    assert!(!shell.contains("--read-only"), "{shell}");
    assert!(shell.contains("--pin-folder"), "{shell}");
    let policy = policy(
        &call,
        &w.path("login"),
        &Files::of(&call),
        Path::new("/nowhere/codex"),
        &[],
    );
    assert!(policy.write.contains(&w.path("wt")), "{:?}", policy.write);
}

#[test]
fn a_call_with_no_fence_has_no_shell() {
    let w = World::new();
    for tools in [Tools::Review, Tools::Retro] {
        let mut call = reviewer(&w, None);
        call.tools = tools;

        let argv = strings(&call, None, None);

        assert!(shell_is_off(&argv), "{tools:?}");
        assert_eq!(hook_for(&argv, "^Bash$"), None, "{tools:?}");
    }
}

#[test]
fn a_reviewer_is_told_what_its_shell_runs_and_a_retro_is_left_alone() {
    let w = World::new();
    let mut call = reviewer(&w, Some(&w.path("build/review-read")));
    call.reach.fence.as_mut().unwrap().write.clear();
    call.prompt = "Review the change.".into();

    let prompt = strings(&call, None, None).pop().unwrap();

    assert!(prompt.starts_with("Review the change."), "{prompt}");
    for command in ["cat", "rg", "find", "nl"] {
        assert!(prompt.contains(&format!("`{command}`")), "{prompt}");
    }
    assert!(prompt.contains("no Read, Glob or Grep"), "{prompt}");

    call.tools = Tools::Retro;
    assert_eq!(
        strings(&call, None, None).pop().unwrap(),
        "Review the change."
    );
}

#[test]
fn a_codex_triager_has_a_read_only_shell_and_no_patch() {
    let w = World::new();
    let mut call = reviewer(&w, Some(&w.path("build/review-read")));
    call.role = Role::Triager;
    call.reach.fence.as_mut().unwrap().write.clear();

    let argv = strings(&call, None, None);

    assert!(!shell_is_off(&argv));
    let shell = hook_for(&argv, "^Bash$").expect("a hook judges the shell");
    assert!(shell.contains("'--read-only'"), "{shell}");
    refuses_every_patch(&hook_for(&argv, "^apply_patch$").expect("a hook holds apply_patch"));
}
