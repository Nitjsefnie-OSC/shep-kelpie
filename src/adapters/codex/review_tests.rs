//! A Codex reviewer asked for a failing test: `apply_patch`, held to its tests folder

use std::path::{Path, PathBuf};

use super::tests::{WORKER_ID, World, codex_hooks, id, strings};
use super::*;
use crate::ports::Role;

// A reviewer's call as the runner shapes it: the fence it gets with a tests
// folder, whose write folder, guard worktree and guard build are that folder.
fn reviewer(w: &World, tests: Option<&Path>) -> AgentCall {
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    call.role = Role::Reviewer;
    call.tools = Tools::Review;
    match tests {
        Some(folder) => {
            let fence = call.reach.fence.as_mut().unwrap();
            fence.write = vec![folder.to_owned()];
            fence.guard.worktree = folder.to_owned();
            fence.guard.build = folder.to_owned();
        }
        None => call.reach.fence = None,
    }
    call
}

#[test]
fn a_codex_reviewer_asked_for_tests_may_patch_its_tests_folder_alone() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let call = reviewer(&w, Some(&folder));

    let argv = strings(&call, None, None);

    assert!(argv.windows(2).any(|a| a == ["--disable", "shell_tool"]));
    let hooks = codex_hooks(&argv, "PreToolUse").unwrap();
    let patch = hooks
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["matcher"].as_str() == Some("^apply_patch$"))
        .expect("a hook holds apply_patch");
    let folder_arg = format!("'{}'", folder.display());
    assert_eq!(
        patch["hooks"][0]["command"].as_str().unwrap(),
        format!("'/k/kelpie' 'confine' {folder_arg} {folder_arg}")
    );

    let policy = policy(&call, &w.path("login"), &Files::of(&call), &[]);
    assert!(policy.write.contains(&folder), "{:?}", policy.write);
    let worktree = w.path("wt");
    let in_worktree: Vec<&PathBuf> = (policy.write.iter())
        .filter(|p| p.starts_with(&worktree) || worktree.starts_with(p))
        .collect();
    assert!(in_worktree.is_empty(), "{in_worktree:?}");
}

#[test]
fn a_codex_reviewer_with_no_tests_folder_may_patch_nothing() {
    let w = World::new();
    let call = reviewer(&w, None);

    let argv = strings(&call, None, None);

    assert!(argv.windows(2).any(|a| a == ["--disable", "shell_tool"]));
    let hooks = codex_hooks(&argv, "PreToolUse").unwrap();
    let hooks = hooks.as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["matcher"].as_str(), Some("^apply_patch$"));
    // The hook refuses as Codex reads a refusal: exit 2, the reason on stderr.
    let refuse = hooks[0]["hooks"][0]["command"].as_str().unwrap();
    let ran = std::process::Command::new("sh")
        .args(["-c", refuse])
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(2));
    assert_eq!(ran.stderr, b"kelpie: this call writes no files\n");
}
