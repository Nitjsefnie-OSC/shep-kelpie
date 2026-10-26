//! A bot file's `review_wait`, and the bot that has never reviewed on the repo

use std::sync::Mutex;

use serde_json::json;

use super::super::{ANSWER_WAIT, REVIEW_WAIT};
use super::{cr, labels, now, off, on, reviewed_by_qwen, skipped};
use crate::runner::{Runner, StepReport, step};
use crate::test::Rig;

const NOT_INSTALLED: &str =
    "it has never posted a review on this repo, so it may not be installed there";

// CodeRabbit's round summoned, its file setting `review_wait` in minutes
// when given, and the forge saying it reviewed before.
fn summoned_with(minutes: Option<u32>) -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = reviewed_by_qwen("shep");
    let wait = minutes.map_or(String::new(), |m| format!("review_wait: {m}\n"));
    let file = format!(
        "---\nrole: reviewer\nharness: bot\nbot: coderabbit\nreviews: 1\nhours: 1\n{wait}---\n"
    );
    rig.write_agent("coderabbit", &file);
    drop(runner);
    let runner = rig.open().unwrap();
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::MarkedReady { .. })
    ));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Summoned { .. })
    ));
    (rig, runner, head)
}

fn silent(head: &str) -> Option<StepReport> {
    skipped(&format!(
        "it never reviewed {} in the wait its agent file allows after its summon",
        &head[..7]
    ))
}

// Steps at `wait - 1` seconds after the summon, where the summon sent once
// more goes out, and then at `wait`, where the bot is passed over.
fn passed_over_at(rig: &Rig, runner: &Mutex<Runner>, head: &str, wait: u64) {
    rig.clock.advance(wait - 1);
    assert!(matches!(
        step(runner).unwrap(),
        Some(StepReport::SummonedAgain { .. })
    ));
    assert_eq!(step(runner).unwrap(), None, "still inside the wait");
    rig.clock.advance(1);
    assert_eq!(step(runner).unwrap(), silent(head));
    assert_eq!(labels(rig).last(), Some(&off()));
}

#[test]
fn a_silent_bot_is_passed_over_at_the_review_wait_its_file_sets() {
    let (rig, runner, head) = summoned_with(Some(20));
    passed_over_at(&rig, &runner, &head, 20 * 60);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["work_item"]["bots_skipped"],
        json!([{ "why": "silent", "reviewer": "coderabbit", "head": head }])
    );
}

#[test]
fn a_silent_bot_whose_file_sets_no_review_wait_is_passed_over_at_45_minutes() {
    assert_eq!(REVIEW_WAIT, 45 * 60);
    let (rig, runner, head) = summoned_with(None);
    passed_over_at(&rig, &runner, &head, REVIEW_WAIT);
}

#[test]
fn a_bot_that_never_reviewed_on_the_repo_is_passed_over_once_its_summon_is_spent() {
    let (rig, runner, _) = summoned_with(None);
    rig.forge.set_last_bot_review(None);
    rig.clock.advance(ANSWER_WAIT - 1);
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "its summon may still be heard"
    );
    rig.clock.advance(1);
    assert_eq!(step(&runner).unwrap(), skipped(NOT_INSTALLED));
    assert!(!rig.leases.held(&cr()));
    assert_eq!(labels(&rig).last(), Some(&off()));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["work_item"]["bots_skipped"],
        json!([{ "why": "not-installed", "reviewer": "coderabbit" }])
    );
}

#[test]
fn a_bot_that_reviewed_before_is_not_passed_over_for_being_silent_ten_minutes() {
    let (rig, runner, _) = summoned_with(None);
    rig.clock.advance(ANSWER_WAIT);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(labels(&rig), [on()], "still summoned");
}

// A bot that shows a sign it is reviewing is not one that is not installed,
// and waits for its review as it did.
#[test]
fn a_bot_that_is_still_reviewing_waits_out_its_review_wait_though_it_never_reviewed_before() {
    let (rig, runner, head) = summoned_with(Some(20));
    rig.forge.set_last_bot_review(None);
    rig.forge.coderabbit.start(71, now(&rig) + 20);
    rig.clock.advance(ANSWER_WAIT + 20);
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(20 * 60 - ANSWER_WAIT - 21);
    assert_eq!(step(&runner).unwrap(), None, "reviewing inside the wait");
    rig.clock.advance(1);
    assert_eq!(step(&runner).unwrap(), silent(&head));
}
