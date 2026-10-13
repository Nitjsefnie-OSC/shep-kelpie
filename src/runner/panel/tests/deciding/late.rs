//! A ruling the maintainer answers while its panel is still voting: under
//! `shadow` the votes are still recorded against the answer, under `panel`
//! the answer ends the vote

use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;

use super::{PANEL, SHADOW, leak, pm_says, ruling, says, state_of};
use crate::ports::Role;
use crate::runner::{Runner, step};
use crate::settings::Merging;
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

/// A merge ruling under `rulings` whose advisor is held mid-vote while the
/// worker, the project manager and the researcher vote A. Returns once the
/// advisor's call is under way, and the rig, runner and the advisor's hold.
fn advisor_held(project: &str, rulings: &str) -> (Rig, Mutex<Runner>, Hold) {
    let (rig, runner) = ruling(project, "feat: add it", Merging::Auto, rulings);
    let hold = Hold::default();
    let reply = leak(says("ADVISOR", 'B', false));
    rig.claude
        .script_for(Role::Advisor, [Scripted::HoldThenSay(hold.clone(), reply)]);
    rig.claude.script_for(
        Role::Worker,
        [Scripted::Say(leak(says("WORKER", 'A', false)))],
    );
    rig.claude.script_for(
        Role::Researcher,
        [Scripted::Say(leak(says("RESEARCHER", 'A', false)))],
    );
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(leak(pm_says('A', false)))]);
    let deadline = Instant::now() + CALL_PATIENCE;
    while !hold.entered(std::time::Duration::from_millis(10)) {
        assert!(Instant::now() < deadline, "the advisor was never asked");
        step(&runner).unwrap();
    }
    (rig, runner, hold)
}

/// Steps until `done` holds of ruling 71's line in `rulings.jsonl`
fn until_line(rig: &Rig, runner: &Mutex<Runner>, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + CALL_PATIENCE;
    while Instant::now() < deadline {
        step(runner).unwrap();
        if let Some(line) = line_of(rig).filter(&done) {
            return line;
        }
        std::thread::yield_now();
    }
    panic!("the line never came to be: {:?}", line_of(rig));
}

/// Ruling 71's line in `rulings.jsonl`, once the file has one
fn line_of(rig: &Rig) -> Option<Value> {
    let text = std::fs::read_to_string(rig.paths().rulings).ok()?;
    (text.lines())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line["id"] == 71)
}

#[test]
fn a_shadow_vote_finished_after_the_answer_is_recorded_against_it() {
    let (rig, runner, hold) = advisor_held("late-recorded", SHADOW);

    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(line_of(&rig).unwrap()["answer"], "yes");
    hold.release();
    let line = until_line(&rig, &runner, |l| !l["ballots"].is_null());

    let ballots = line["ballots"].as_array().unwrap();
    assert_eq!(ballots.len(), 4, "{line}");
    let advisor = ballots.iter().find(|b| b["voter"] == "advisor").unwrap();
    assert_eq!(advisor["vote"]["choice"], "B", "{line}");
    assert_eq!(line["verdict"]["shadow"], true, "{line}");
    assert_eq!(line["answer"], "yes", "{line}");
    assert_eq!(line["chose"], "A", "{line}");
}

#[test]
fn a_late_shadow_vote_does_not_change_what_the_answer_did() {
    let (rig, runner, hold) = advisor_held("late-unchanged", SHADOW);
    rig.ask(&runner, "rule", Some("71 yes"));
    for _ in 0..5 {
        step(&runner).unwrap();
    }
    let answered = state_of(&rig);
    let merges = rig.forge.merges();
    assert_eq!(answered["rulings"].as_array().map(Vec::len), Some(0));

    hold.release();
    until_line(&rig, &runner, |l| !l["ballots"].is_null());
    for _ in 0..5 {
        step(&runner).unwrap();
    }

    let after = state_of(&rig);
    assert_eq!(after["rulings"], answered["rulings"], "no ruling came back");
    assert_eq!(
        after["work_items"][0]["phase"],
        answered["work_items"][0]["phase"]
    );
    assert_eq!(rig.forge.merges(), merges);
    let asking = (rig.github.comments().into_iter())
        .any(|(_, body)| body.starts_with("### Merge this pull request"));
    assert!(
        !asking,
        "the answered ruling was not posted to the maintainer"
    );
}

#[test]
fn status_counts_a_ruling_answered_before_the_panel_finished() {
    let (rig, runner, hold) = advisor_held("late-status", SHADOW);
    rig.ask(&runner, "rule", Some("71 yes"));
    hold.release();
    until_line(&rig, &runner, |l| !l["ballots"].is_null());

    let panel = rig.ask(&runner, "status", None)["panel"].clone();

    assert_eq!(panel["voted"], 1, "{panel}");
    assert_eq!(panel["answered"], 1, "{panel}");
    assert_eq!(panel["experts"]["advisor"]["votes"], 1, "{panel}");
    assert_eq!(panel["experts"]["advisor"]["agreed"], 0, "{panel}");
    assert_eq!(panel["experts"]["worker"]["agreed"], 1, "{panel}");
}

#[test]
fn under_panel_an_early_answer_still_ends_the_vote() {
    let (rig, runner, hold) = advisor_held("late-panel", PANEL);

    rig.ask(&runner, "rule", Some("71 yes"));
    let deadline = Instant::now() + CALL_PATIENCE;
    while !hold.returned() {
        assert!(Instant::now() < deadline, "the advisor's call ran on");
        step(&runner).unwrap();
        std::thread::yield_now();
    }

    for _ in 0..5 {
        step(&runner).unwrap();
    }
    let line = line_of(&rig).unwrap();
    assert!(line["ballots"].is_null(), "no votes were kept: {line}");
    assert!(line["verdict"].is_null(), "{line}");
}
