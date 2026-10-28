//! A goose reviewer asked for a failing test: `write`, held to its tests folder

#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use serde_json::{Value, json};

use super::hooks_tests::{plugin, policy_of, worker};
use super::tests::{FRESH, FRESH_ID, World, id, stand_in};
use super::*;
use crate::confine::Verdict;
use crate::guard::{
    goose_server_flags, goose_write_flag, judge_goose_reading, judge_goose_writing,
};
use crate::hooks::Hooks;

// A reviewer's call as the runner shapes it: with a tests folder, the fence's
// write folder, guard worktree and guard build are that folder, and with none
// the fence writes nothing.
fn reviewer(w: &World, tests: Option<&Path>) -> AgentCall {
    let mut call = AgentCall {
        role: Role::Reviewer,
        tools: Tools::Review,
        ..worker(w, Session::New(id(FRESH_ID)))
    };
    let fence = call.reach.fence.as_mut().unwrap();
    match tests {
        Some(folder) => {
            std::fs::create_dir_all(folder).unwrap();
            fence.write = vec![folder.to_owned()];
            fence.guard.worktree = folder.to_owned();
            fence.guard.build = folder.to_owned();
        }
        None => fence.write.clear(),
    }
    call
}

// The arguments goose's hook command runs kelpie with, from a stand-in that keeps them.
fn hook_args(w: &World, call: &mut AgentCall) -> Vec<String> {
    let kelpie = w.path("kelpie-bin");
    let seen = w.path("seen");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", seen.display());
    crate::test::write_script(&kelpie, &script);
    call.reach.fence.as_mut().unwrap().guard.kelpie = kelpie;
    stand_in(w, FRESH).prepare(call).unwrap();

    let hooks = super::hooks_tests::json_in(&plugin(w).join("hooks/hooks.json"));
    let command = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let ran = Process::new("sh").args(["-c", command]).status().unwrap();
    assert!(ran.success());
    let seen = std::fs::read_to_string(&seen).unwrap();
    seen.lines().map(str::to_owned).collect()
}

fn payload(tool: &str, input: Value, working_dir: &Path) -> String {
    json!({
        "event": "PreToolUse",
        "session_id": "20260101_1",
        "tool_name": tool,
        "tool_input": input,
        "working_dir": working_dir,
    })
    .to_string()
}

// The verdict of the hook the call was given, as `kelpie guard` reads its arguments.
fn judged(args: &[String], tool: &str, input: Value, working_dir: &Path) -> Verdict {
    let (_, rest) = crate::guard::goose_flag(args);
    let (folder, rest) = goose_write_flag(&rest);
    let (servers, _) = goose_server_flags(&rest);
    let text = payload(tool, input, working_dir);
    match folder {
        Some(folder) => judge_goose_writing(text.as_bytes(), &Hooks::default(), &servers, &folder),
        None => judge_goose_reading(text.as_bytes(), &Hooks::default(), &servers),
    }
}

fn write_of(path: impl AsRef<Path>) -> Value {
    json!({ "path": path.as_ref(), "content": "#[test]\nfn fails() {}\n" })
}

#[test]
fn a_goose_reviewer_asked_for_tests_may_write_its_tests_folder_alone() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let mut call = reviewer(&w, Some(&folder));

    let args = hook_args(&w, &mut call);

    // The hook names the tests folder for `write`, and keeps the read-only guard.
    assert!(args.contains(&"--read-only".to_owned()), "{args:?}");
    let (named, _) = goose_write_flag(&args);
    assert_eq!(named, Some(folder.clone()));
    let worktree = w.path("wt");
    let ok = judged(
        &args,
        "write",
        write_of(folder.join("tests/flag.rs")),
        &worktree,
    );
    assert_eq!(ok, Verdict::Allow);
    // `edit` and every other tool stay refused, and the shell stays read-only.
    for tool in ["edit", "read_image", "delete"] {
        let input = json!({ "path": folder.join("a.rs"), "before": "a", "after": "b" });
        let verdict = judged(&args, tool, input, &worktree);
        assert!(matches!(verdict, Verdict::Refuse(_)), "{tool}: {verdict:?}");
    }
    let redirect = json!({ "command": "echo x > a.rs" });
    let verdict = judged(&args, "shell", redirect, &worktree);
    assert!(matches!(verdict, Verdict::Refuse(_)), "{verdict:?}");
    let read = json!({ "command": "cat src/lib.rs" });
    assert_eq!(judged(&args, "shell", read, &worktree), Verdict::Allow);

    // The sandbox holds that folder and no part of the worktree.
    let policy = policy_of(&w, &call);
    assert!(policy.write.contains(&folder), "{:?}", policy.write);
    let in_worktree: Vec<&PathBuf> = (policy.write.iter())
        .filter(|p| p.starts_with(&worktree) || worktree.starts_with(p))
        .collect();
    assert!(in_worktree.is_empty(), "{in_worktree:?}");
}

#[test]
fn a_goose_reviewer_with_no_tests_folder_stays_read_only() {
    let w = World::new();
    let mut call = reviewer(&w, None);

    let args = hook_args(&w, &mut call);

    assert_eq!(goose_write_flag(&args).0, None, "{args:?}");
    for tool in ["write", "edit"] {
        let input =
            json!({ "path": w.path("build/review-tests/a.rs"), "content": "x", "after": "x" });
        let Verdict::Refuse(why) = judged(&args, tool, input, &w.path("wt")) else {
            panic!("{tool} was let through");
        };
        assert!(why.contains("writes no file"), "{tool}: {why}");
    }
}

#[test]
fn a_retro_with_a_write_folder_gets_no_write() {
    let w = World::new();
    let mut call = AgentCall {
        tools: Tools::Retro,
        ..worker(&w, Session::New(id(FRESH_ID)))
    };
    assert!(!call.reach.fence.as_ref().unwrap().write.is_empty());

    let args = hook_args(&w, &mut call);

    assert_eq!(goose_write_flag(&args).0, None, "{args:?}");
}

#[test]
fn the_hook_resolves_a_path_as_goose_does() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let mut call = reviewer(&w, Some(&folder));
    let args = hook_args(&w, &mut call);
    let worktree = w.path("wt");
    let judge =
        |path: &str, working_dir: &Path| judged(&args, "write", write_of(path), working_dir);

    // A relative path joins the call's working folder: the tests folder holds it
    // when the call runs there, and the worktree does not.
    assert_eq!(judge("tests/flag.rs", &folder), Verdict::Allow);
    assert!(matches!(
        judge("tests/flag.rs", &worktree),
        Verdict::Refuse(_)
    ));
    // An absolute path is used as it is, wherever the call runs.
    let inside = folder.join("tests/flag.rs");
    assert_eq!(judge(inside.to_str().unwrap(), &worktree), Verdict::Allow);
    let outside = worktree.join("tests/flag.rs");
    assert!(matches!(
        judge(outside.to_str().unwrap(), &folder),
        Verdict::Refuse(_)
    ));
    // `~` is a folder's name, not the home: inside the working folder it stays
    // inside the tests folder, and inside the worktree it is refused.
    assert_eq!(judge("~/flag.rs", &folder), Verdict::Allow);
    assert!(matches!(judge("~/flag.rs", &worktree), Verdict::Refuse(_)));
    // A climb out of the tests folder is refused, written absolute or relative.
    let climb = format!("{}/../../wt/flag.rs", folder.display());
    assert!(matches!(judge(&climb, &worktree), Verdict::Refuse(_)));
    assert!(matches!(
        judge("../../wt/flag.rs", &folder),
        Verdict::Refuse(_)
    ));
}

#[cfg(unix)]
#[test]
fn a_write_through_a_link_in_the_tests_folder_to_the_worktree_is_refused() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let mut call = reviewer(&w, Some(&folder));
    let args = hook_args(&w, &mut call);
    let worktree = w.path("wt");
    std::fs::create_dir_all(&worktree).unwrap();
    // A link the test makes, as a reviewer with a shell that writes could.
    symlink(&worktree, folder.join("link")).unwrap();

    for path in ["link/tests/flag.rs", "link/flag.rs"] {
        let given = folder.join(path);
        let verdict = judged(&args, "write", write_of(&given), &worktree);
        assert!(matches!(verdict, Verdict::Refuse(_)), "{path}: {verdict:?}");
        let relative = judged(&args, "write", write_of(path), &folder);
        assert!(
            matches!(relative, Verdict::Refuse(_)),
            "{path}: {relative:?}"
        );
    }
    assert!(!worktree.join("flag.rs").exists());
}
