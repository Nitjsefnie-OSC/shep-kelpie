//! A ruling the maintainer answers while its panel is still voting: under
//! `shadow` the votes are still recorded against the answer, under `panel`
//! the answer ends the vote

use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;

use super::{PANEL, SHADOW, leak, pm_says, ruling, says, state_of};
use crate::ports::{AgentError, Cost, Role};
use crate::runner::{Runner, step};
use crate::settings::Merging;
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

/// A merge ruling under `rulings` whose `held` expert is held mid-vote while
/// the others vote A. Returns once its call is under way, with the rig,
/// runner and its hold.
fn held(project: &str, rulings: &str, held: Role) -> (Rig, Mutex<Runner>, Hold) {
    let cost = Cost::from_usd(0.25).unwrap();
    let reply = leak(says("WORKER", 'A', false));
    voting(project, rulings, held, Scripted::Billed(reply, cost))
}

/// [`held`], with the worker's call scripted as `worker` unless it is held
fn voting(
    project: &str,
    rulings: &str,
    held: Role,
    worker: Scripted,
) -> (Rig, Mutex<Runner>, Hold) {
    let (rig, runner) = ruling(project, "feat: add it", Merging::Auto, rulings);
    let hold = Hold::default();
    let mut worker = Some(worker);
    for (role, who) in [
        (Role::Worker, "WORKER"),
        (Role::Researcher, "RESEARCHER"),
        (Role::Advisor, "ADVISOR"),
    ] {
        let choice = if role == Role::Advisor { 'B' } else { 'A' };
        let reply = leak(says(who, choice, false));
        let script = match (role == held, role) {
            (true, _) => Scripted::HoldThenSay(hold.clone(), reply),
            (_, Role::Worker) => worker.take().unwrap(),
            _ => Scripted::Say(reply),
        };
        rig.claude.script_for(role, [script]);
    }
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(leak(pm_says('A', false)))]);
    let deadline = Instant::now() + CALL_PATIENCE;
    while !hold.entered(std::time::Duration::from_millis(10)) {
        assert!(Instant::now() < deadline, "the held expert was never asked");
        step(&runner).unwrap();
    }
    (rig, runner, hold)
}

fn advisor_held(project: &str, rulings: &str) -> (Rig, Mutex<Runner>, Hold) {
    held(project, rulings, Role::Advisor)
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

#[test]
fn a_worker_ended_by_the_answer_abstains_as_overtaken_not_as_a_failed_call() {
    let (rig, runner, _hold) = held("late-worker", SHADOW, Role::Worker);

    rig.ask(&runner, "rule", Some("71 yes"));
    let line = until_line(&rig, &runner, |l| !l["ballots"].is_null());

    let ballots = line["ballots"].as_array().unwrap();
    let worker = ballots.iter().find(|b| b["voter"] == "worker").unwrap();
    assert!(worker["vote"].is_null(), "{line}");
    assert_eq!(line["overtaken"], serde_json::json!(["worker"]), "{line}");
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("the worker abstains, since the maintainer answered the ruling first"),
        "{notes}"
    );
    let experts = rig.ask(&runner, "status", None)["panel"]["experts"].clone();
    assert_eq!(experts["worker"]["overtaken"], 1, "{experts}");
    assert_eq!(experts["worker"]["abstained"], 0, "{experts}");
    assert_eq!(experts["advisor"]["votes"], 1, "{experts}");
}

/// Every line of the project's usage ledger, as JSON
fn ledger(rig: &Rig) -> Vec<Value> {
    let lines = crate::usage::read(&rig.paths().folder.join(crate::usage::FILE)).unwrap();
    (lines.iter())
        .map(|line| serde_json::to_value(line).unwrap())
        .collect()
}

#[test]
fn a_worker_vote_is_in_the_ledger_before_the_answered_item_is_freed() {
    let (rig, runner, hold) = advisor_held("late-ledger", SHADOW);
    rig.ask(&runner, "rule", Some("71 yes"));
    for _ in 0..3 {
        step(&runner).unwrap();
    }

    let before = ledger(&rig);
    let vote = before
        .iter()
        .find(|l| l["votes"][0]["voter"] == "worker")
        .unwrap_or_else(|| panic!("no line for the worker's vote: {before:#?}"));
    assert_eq!(vote["cost_usd"], 0.25, "{vote}");
    hold.release();
    until_line(&rig, &runner, |l| !l["ballots"].is_null());
    let after = ledger(&rig);
    let lines = after.iter().filter(|l| l["votes"][0]["voter"] == "worker");
    assert_eq!(lines.count(), 1, "the worker's vote is lined once");
}

#[test]
fn a_worker_whose_call_failed_before_the_answer_is_not_overtaken() {
    let failed = Scripted::Fail(AgentError::Setup("no session".into()));
    let (rig, runner, hold) = voting("late-failed", SHADOW, Role::Advisor, failed);
    for _ in 0..3 {
        step(&runner).unwrap();
    }
    rig.ask(&runner, "rule", Some("71 yes"));

    hold.release();
    let line = until_line(&rig, &runner, |l| !l["ballots"].is_null());

    assert!(line["overtaken"].is_null(), "{line}");
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("the worker abstains, since its call failed"),
        "{notes}"
    );
}
