//! A pi reviewer asked for a failing test: `write`, held to its tests folder

use std::path::{Path, PathBuf};

use serde_json::json;

use super::guard_tests::{guarded, has_node};
use super::tests::{FRESH_ID, URL, World, id};
use super::*;
use crate::ports::Role;

// A reviewer's call as the runner shapes it: the fence it gets with a tests
// folder, whose write folder, guard worktree and guard build are that folder.
fn reviewer(w: &World, tests: Option<&Path>) -> AgentCall {
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(FRESH_ID)));
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

fn flag_after(argv: &[OsString], flag: &str) -> Option<String> {
    let at = argv.iter().position(|a| a == flag)?;
    Some(argv[at + 1].to_str().unwrap().to_owned())
}

#[test]
fn a_pi_reviewer_asked_for_tests_gets_write_and_the_guard() {
    let w = World::new();
    let folder = w.path("build/review-tests");
    let call = reviewer(&w, Some(&folder));

    let argv = argv(&call, &Files::of(&call));

    assert_eq!(
        flag_after(&argv, "--tools").as_deref(),
        Some("read,grep,find,ls,write")
    );
    assert_eq!(
        flag_after(&argv, "--extension"),
        Some(w.path("settings.guard.ts").display().to_string())
    );
    let fence = call.reach.fence.as_deref().unwrap();
    let text = guard_extension(fence, false);
    let line = text
        .lines()
        .find(|l| l.starts_with("const CHECKS"))
        .unwrap();
    let checks: serde_json::Value =
        serde_json::from_str(line.split_once(" = ").unwrap().1.trim_end_matches(';')).unwrap();
    assert_eq!(checks["confine"], json!(["confine", folder, folder]));

    let socket = w.path("forwarder.sock");
    let upstream = PiCli::on(&Base::default())
        .upstream(server(&call).unwrap())
        .unwrap();
    let policy = policy(
        &call,
        &Files::of(&call),
        &upstream,
        &socket,
        Path::new("/nowhere/pi"),
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
fn a_pi_reviewer_with_no_tests_folder_stays_read_only() {
    let w = World::new();
    let call = reviewer(&w, None);

    let argv = argv(&call, &Files::of(&call));

    assert_eq!(
        flag_after(&argv, "--tools").as_deref(),
        Some("read,grep,find,ls")
    );
    assert_eq!(flag_after(&argv, "--extension"), None);
}

#[test]
fn the_guard_hands_a_reviewers_write_to_confine_as_an_absolute_path() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    let log = w.path("checks.log");
    let script = format!(
        "#!/bin/sh\ncat >> {log}\necho >> {log}\n",
        log = log.display()
    );
    crate::test::write_script(&w.path("kelpie"), &script);
    let home = std::env::var("HOME").expect("a home to expand ~ to");
    let events = json!([
        { "toolName": "write", "input": { "path": "tests/relative.rs", "content": "a" } },
        { "toolName": "write", "input": { "path": "@tests/at.rs", "content": "b" } },
        { "toolName": "write", "input": { "path": "~/tests/home.rs", "content": "c" } },
        { "toolName": "write", "input": { "path": "file:///etc/hosts", "content": "d" } },
    ]);

    let verdicts = guarded(&w, &w.path("kelpie"), events);

    let blocked: Vec<bool> = verdicts.iter().map(|v| v["block"] == true).collect();
    assert_eq!(blocked, [false, false, false, true]);
    assert!(
        verdicts[3]["reason"]
            .as_str()
            .unwrap()
            .contains("file: URL")
    );
    let sent = std::fs::read_to_string(&log).unwrap();
    let paths: Vec<String> = sent
        .lines()
        .map(|l| {
            let call: serde_json::Value = serde_json::from_str(l).unwrap();
            assert_eq!(call["tool_name"], "Write");
            call["tool_input"]["file_path"].as_str().unwrap().to_owned()
        })
        .collect();
    let wt = w.path("wt");
    assert_eq!(
        paths,
        [
            wt.join("tests/relative.rs").display().to_string(),
            wt.join("tests/at.rs").display().to_string(),
            format!("{home}/tests/home.rs"),
        ]
    );
}

#[test]
fn a_pi_triager_reads_with_no_write_and_no_shell() {
    let w = World::new();
    let mut call = reviewer(&w, None);
    call.role = Role::Triager;

    let argv = argv(&call, &Files::of(&call));

    assert_eq!(
        flag_after(&argv, "--tools").as_deref(),
        Some("read,grep,find,ls")
    );
    assert_eq!(flag_after(&argv, "--extension"), None);
}
