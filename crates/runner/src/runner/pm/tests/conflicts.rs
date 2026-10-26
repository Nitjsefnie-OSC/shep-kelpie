//! A conflict wakes the project manager only when two work items' own changes
//! clash, never for a branch behind `main`

use std::sync::Mutex;

use crate::ports::Role;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted, git};

use super::{NOTHING, answered, name_pm, past_the_window, pm_seen, step_until};

/// Issues 7 and 8 open with pull requests 71 and 81, each worker having pushed
/// its own `README.md`, then `main` landing `file` as `text`
fn two_readmes_then_main(file: &str, text: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("acme");
    name_pm(&rig);
    rig.edit_settings(|s| s.replace("active_items = 1", "active_items = 3"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.forge.open_pull_request(81, "kelpie/8", &[8]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.ask(&runner, "add", Some("8"));
    rig.claude.script([
        Scripted::Push("README.md", "seven\n"),
        Scripted::Push("README.md", "eight\n"),
    ]);
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    rig.land(file, text);
    git(&rig.repo(), &["fetch", "--quiet", "origin"]);
    (rig, runner)
}

fn board(rig: &Rig) -> String {
    std::fs::read_to_string(rig.paths().board).unwrap()
}

#[test]
fn a_branch_behind_main_reads_as_behind_once_and_wakes_nobody() {
    // `main` holds #7's `README.md`, so only #8 conflicts with it.
    let (rig, runner) = two_readmes_then_main("README.md", "seven\n");
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
    let (rig, runner) = two_readmes_then_main("CHANGES.md", "landed\n");
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
