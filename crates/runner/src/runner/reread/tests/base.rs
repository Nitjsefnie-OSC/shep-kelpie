//! A `git.base` changed while the runner runs: the work items opened after
//! the change are cut from the new base, and each one in flight finishes on
//! the base it was cut for

use std::fs;
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::ports::Checks;
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted, git, ticket};

// Makes `dev` the base the settings name, and has the running runner read them.
fn base_moves_to_dev(rig: &Rig, runner: &Mutex<Runner>) -> Option<String> {
    rig.base_is("dev");
    let mut runner = runner.lock().unwrap();
    runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap()
}

// The worker of issue `n` pushes, leaving the file kelpie opens its pull request from.
fn opens_a_pull_request(rig: &Rig, runner: &Mutex<Runner>, n: u64) -> u64 {
    let build = rig.paths().build(ticket(n));
    fs::create_dir_all(&build).unwrap();
    let body = format!("feat: x\n\nWhy.\n\nResolves #{n}\n");
    fs::write(build.join("pull-request.md"), body).unwrap();
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(runner).unwrap();
    rig.ask(runner, "status", None)["work_item"]["pull_request"]
        .as_u64()
        .expect("a pull request was opened")
}

// Whether issue `n`'s worktree is one commit on from `origin/<base>`.
fn is_cut_from(rig: &Rig, n: u64, base: &str) -> bool {
    let worktree = rig.paths().worktree(ticket(n));
    let from = git(&rig.repo(), &["rev-parse", &format!("origin/{base}")]);
    git(&worktree, &["rev-parse", "HEAD~1"]) == from
}

// The state file the runner saved, as JSON.
fn saved(rig: &Rig) -> Value {
    serde_json::from_str(&fs::read_to_string(rig.paths().state).unwrap()).unwrap()
}

#[test]
fn a_changed_base_is_in_effect_at_once() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));

    let line = base_moves_to_dev(&rig, &runner);

    assert_eq!(
        line.as_deref(),
        Some("settings changed: git.base now in effect")
    );
    assert_eq!(runner.lock().unwrap().settings().git.base(), "dev");
}

#[test]
fn a_work_item_in_flight_finishes_on_the_base_it_was_cut_from() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.merge_auto();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    base_moves_to_dev(&rig, &runner);
    let dev = rig.forge.head_of("dev");

    let number = opens_a_pull_request(&rig, &runner, 7);

    assert!(
        is_cut_from(&rig, 7, "main"),
        "the worktree is cut from main"
    );
    assert_eq!(rig.forge.base_of(number), "main");
    rig.claude.script([
        Scripted::Text(crate::ports::CLEAN),
        Scripted::Text(crate::ports::CLEAN),
    ]);
    for _ in 0..40 {
        if let Some(head) = rig.forge.head_of("kelpie/7") {
            rig.forge.set_checks(&head, Checks::Passed);
        }
        rig.verdict(&runner);
        if !rig.forge.merges().is_empty() {
            break;
        }
    }
    let [(merged, _)] = rig.forge.merges().try_into().expect("one merge");
    assert_eq!(merged, number);
    assert_eq!(rig.forge.base_of(merged), "main");
    assert_eq!(rig.forge.head_of("dev"), dev, "dev is left alone");
}

#[test]
fn the_next_work_item_is_cut_from_the_new_base_and_its_pull_request_targets_it() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    base_moves_to_dev(&rig, &runner);
    rig.ask(&runner, "drop", None);
    rig.ask(&runner, "add", Some("8"));

    let number = opens_a_pull_request(&rig, &runner, 8);

    assert!(is_cut_from(&rig, 8, "dev"), "the worktree is cut from dev");
    assert_eq!(rig.forge.base_of(number), "dev");
}

#[test]
fn a_restart_keeps_the_work_item_in_flight_on_its_base() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    base_moves_to_dev(&rig, &runner);
    drop(runner);
    assert_eq!(saved(&rig)["work_items"][0]["base"], "main");

    let runner = rig.open().unwrap();
    assert_eq!(runner.lock().unwrap().settings().git.base(), "dev");
    let number = opens_a_pull_request(&rig, &runner, 7);

    assert!(
        is_cut_from(&rig, 7, "main"),
        "the worktree is cut from main"
    );
    assert_eq!(rig.forge.base_of(number), "main");
}

#[test]
fn a_work_item_saved_with_no_base_takes_the_base_the_runner_starts_with() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    drop(runner);
    let mut older = saved(&rig);
    older["work_items"][0]
        .as_object_mut()
        .unwrap()
        .remove("base");
    fs::write(rig.paths().state, older.to_string()).unwrap();
    rig.base_is("dev");

    let runner = rig.open().unwrap();
    let number = opens_a_pull_request(&rig, &runner, 7);

    assert!(is_cut_from(&rig, 7, "dev"), "the worktree is cut from dev");
    assert_eq!(rig.forge.base_of(number), "dev");
    assert_eq!(saved(&rig)["work_items"][0]["base"], json!("dev"));
}
