//! A conflict wakes the project manager only when two work items' own changes
//! clash, never for a branch behind `main`

use std::sync::Mutex;

use crate::ports::Role;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted, git};

use super::{NOTHING, answered, name_pm, past_the_window, pm_seen, step_until};

/// Issues 7 and 8 open with pull requests 71 and 81, each worker having done
/// the turn in `work`, then `main` landing `file` as `text`
fn two_branches_then_main(work: [Scripted; 2], file: &str, text: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("acme");
    name_pm(&rig);
    rig.edit_settings(|s| s.replace("active_items = 1", "active_items = 3"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.forge.open_pull_request(81, "kelpie/8", &[8]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.ask(&runner, "add", Some("8"));
    rig.claude.script(work);
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    rig.land(file, text);
    git(&rig.repo(), &["fetch", "--quiet", "origin"]);
    (rig, runner)
}

/// Each worker's turn pushes its own `README.md`
fn readmes() -> [Scripted; 2] {
    [
        Scripted::Push("README.md", "seven\n"),
        Scripted::Push("README.md", "eight\n"),
    ]
}

/// Each worker's turn commits its own `README.md` and `lib.txt`
fn readmes_and_libs() -> [Scripted; 2] {
    [
        Scripted::CommitFiles(&[("README.md", "seven\n"), ("lib.txt", "seven\n")]),
        Scripted::CommitFiles(&[("README.md", "eight\n"), ("lib.txt", "eight\n")]),
    ]
}

fn board(rig: &Rig) -> String {
    std::fs::read_to_string(rig.paths().board).unwrap()
}

#[test]
fn a_branch_behind_main_reads_as_behind_once_and_wakes_nobody() {
    // `main` holds #7's `README.md`, so only #8 conflicts with it.
    let (rig, runner) = two_branches_then_main(readmes(), "README.md", "seven\n");
    rig.claude.script_for(Role::Pm, [Scripted::Say(NOTHING)]);
    rig.clock.advance(5 * 60);
    crate::runner::briefing::brief(&runner);
    let text = board(&rig);
    assert_eq!(
        text.matches("Behind main").count(),
        1,
        "one entry, for #8 alone: {text}"
    );
    let behind = "  - Behind main: conflicts with it in README.md; kelpie catches it up at the \
                  gate, so this is no conflict between work items\n";
    let (seven, eight) = text.split_once("- #8 ").unwrap();
    assert!(!seven.contains(behind), "#7 is clean with main: {text}");
    assert!(eight.contains(behind), "{text}");
    assert!(
        !text.contains("| #7 (PR #71) | #8 (PR #81) | README.md"),
        "{text}"
    );
    past_the_window(&rig);
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    assert!(pm_seen(&rig).is_empty(), "nothing woke the project manager");
}

#[test]
fn two_branches_clean_with_main_that_change_the_same_lines_wake_it() {
    // `main` lands a file neither branch touches.
    let (rig, runner) = two_branches_then_main(readmes(), "CHANGES.md", "landed\n");
    rig.claude.script_for(Role::Pm, [Scripted::Say(NOTHING)]);
    rig.clock.advance(5 * 60);
    crate::runner::briefing::brief(&runner);
    let text = board(&rig);
    assert!(!text.contains("Behind main"), "{text}");
    past_the_window(&rig);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, _, _) = answered(Some(report));
    assert_eq!(
        woke_for,
        ["the branches of #7 and #8 conflict in README.md"],
    );
}

#[test]
fn a_branch_behind_main_still_wakes_it_for_a_clash_that_is_not_mains() {
    // #8 is behind on `README.md`, which #7 holds as `main` does, and clashes
    // with #7 in `lib.txt`, which `main` lacks.
    let (rig, runner) = two_branches_then_main(readmes_and_libs(), "README.md", "seven\n");
    rig.claude.script_for(Role::Pm, [Scripted::Say(NOTHING)]);
    rig.clock.advance(5 * 60);
    crate::runner::briefing::brief(&runner);
    let text = board(&rig);
    assert!(
        text.contains("  - Behind main: conflicts with it in README.md;"),
        "{text}"
    );
    let row = "| #7 (PR #71) | #8 (PR #81) | lib.txt | README.md, lib.txt |";
    assert!(
        text.contains(row),
        "the pair lists only the clash in lib.txt: {text}"
    );
    past_the_window(&rig);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, _, _) = answered(Some(report));
    // The wake was queued before `main` landed, so it may still name both files.
    let [reason] = woke_for.as_slice() else {
        panic!("one wake expected: {woke_for:?}")
    };
    assert!(
        reason.starts_with("the branches of #7 and #8 conflict in"),
        "{reason}"
    );
    assert!(reason.contains("lib.txt"), "{reason}");
}

#[test]
fn a_pair_that_clashes_before_and_after_one_falls_behind_main_wakes_it_once() {
    let (rig, runner) = two_branches_then_main(readmes_and_libs(), "CHANGES.md", "landed\n");
    rig.claude.script_for(Role::Pm, [Scripted::Say(NOTHING)]);
    rig.clock.advance(5 * 60);
    crate::runner::briefing::brief(&runner);
    past_the_window(&rig);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    answered(Some(report));
    // `main` now moves under #8 alone.
    rig.land("README.md", "seven\n");
    git(&rig.repo(), &["fetch", "--quiet", "origin"]);
    rig.claude.script_for(Role::Pm, [Scripted::Say(NOTHING)]);
    rig.clock.advance(5 * 60);
    crate::runner::briefing::brief(&runner);
    assert!(board(&rig).contains("  - Behind main:"));
    past_the_window(&rig);
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    assert_eq!(pm_seen(&rig).len(), 1, "the same clash woke it twice");
}
