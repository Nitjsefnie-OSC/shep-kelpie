//! What the panel's votes leave behind: a ledger line each, and the
//! standing `status` reports once the maintainer answers

use serde_json::{Value, json};

use super::{SHADOW, leak, pm_says, ruling, says, script, voted};
use crate::ports::{Cost, Role};
use crate::settings::Merging;
use crate::test::{Rig, Scripted};
use crate::usage::{FILE, read};

/// Every line of the project's usage ledger, as JSON
fn ledger(rig: &Rig) -> Vec<Value> {
    let lines = read(&rig.paths().folder.join(FILE)).unwrap();
    lines
        .iter()
        .map(|line| serde_json::to_value(line).unwrap())
        .collect()
}

#[test]
fn each_vote_is_a_ledger_line_with_the_ruling_kind_expert_panel_cost_and_wall_time() {
    let (rig, runner) = ruling("ledger", "fix(guard): a bug", Merging::Auto, SHADOW);
    let cost = Cost::from_usd(0.25).unwrap();
    for (role, who) in [
        (Role::Worker, "WORKER"),
        (Role::Advisor, "ADVISOR"),
        (Role::Researcher, "RESEARCHER"),
    ] {
        rig.claude
            .script_for(role, [Scripted::Billed(leak(says(who, 'A', false)), cost)]);
    }
    rig.claude.script_for(
        Role::Pm,
        [Scripted::Billed(leak(pm_says('A', false)), cost)],
    );

    voted(&rig, &runner);

    let lines = ledger(&rig);
    let marked: Vec<&Value> = lines.iter().filter(|l| !l["votes"].is_null()).collect();
    assert_eq!(marked.len(), 4, "one line a vote: {lines:#?}");
    for voter in ["worker", "pm", "researcher", "advisor"] {
        let line = marked
            .iter()
            .find(|l| l["votes"][0]["voter"] == voter)
            .unwrap_or_else(|| panic!("no line for the {voter}: {marked:#?}"));
        assert_eq!(
            line["votes"],
            json!([{
                "ruling": 1, "ruling_kind": "merge", "voter": voter,
                "panel": "fix", "work": "fix", "choice": "A",
            }]),
            "{voter}"
        );
        assert_eq!(line["cost_usd"], json!(0.25), "{voter}");
        assert!(line["seconds"].is_u64(), "{voter}: {line}");
        assert_eq!(line["ended"], "answered", "{voter}");
    }
    let wake = marked.iter().find(|l| l["kind"] == "wake").unwrap();
    assert_eq!(wake["role"], "pm", "the project manager's wake is counted");
    let kinds: Vec<&Value> = marked.iter().map(|l| &l["kind"]).collect();
    assert_eq!(kinds.iter().filter(|k| ***k == "vote").count(), 3);
}

#[test]
fn an_abstaining_expert_has_a_line_with_no_choice() {
    let (rig, runner) = ruling("abstains", "feat: add it", Merging::Auto, SHADOW);
    for (role, who) in [(Role::Worker, "WORKER"), (Role::Researcher, "RESEARCHER")] {
        rig.claude
            .script_for(role, [Scripted::Say(leak(says(who, 'A', false)))]);
    }
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(leak(pm_says('A', false)))]);
    rig.claude
        .script_for(Role::Advisor, [Scripted::Say("I would merge it.")]);

    voted(&rig, &runner);

    let lines = ledger(&rig);
    let advisor = lines
        .iter()
        .find(|l| l["votes"][0]["voter"] == "advisor")
        .expect("a line for the advisor");
    assert!(advisor["votes"][0].get("choice").is_none(), "{advisor}");
}

/// `status`'s panel for the project, once ruling 1 is answered with `answer`
fn standing_after(project: &str, votes: [&str; 4], answer: &str) -> serde_json::Value {
    let (rig, runner) = ruling(project, "feat: add it", Merging::Auto, SHADOW);
    script(&rig, votes);
    voted(&rig, &runner);
    rig.ask(&runner, "rule", Some(answer));
    rig.ask(&runner, "status", None)["panel"].clone()
}

#[test]
fn a_maintainers_answer_that_matches_the_panel_extends_its_streak_and_each_agreeing_expert() {
    let panel = standing_after("agree", ["A", "A", "A", "A"], "1 yes");

    assert_eq!(panel["decide"], "shadow");
    assert_eq!(panel["streak"], 1);
    assert_eq!(panel["pm_alone"]["merge"], 1);
    assert_eq!(panel["experts"]["advisor"]["agreed"], 1);
    assert_eq!(panel["experts"]["advisor"]["lone"], 0);
}

#[test]
fn a_maintainers_answer_that_differs_resets_the_streak_and_the_lone_expert_is_counted() {
    // The researcher alone voted for the no the maintainer gave.
    let panel = standing_after("differ", ["A", "A", "B", "A"], "1 no not yet");

    assert_eq!(panel["streak"], 0);
    assert_eq!(panel["pm_alone"]["merge"], 0);
    assert_eq!(panel["experts"]["researcher"]["agreed"], 1);
    assert_eq!(panel["experts"]["researcher"]["lone"], 1);
    assert_eq!(panel["experts"]["advisor"]["agreed"], 0);
}

#[test]
fn a_project_with_no_panel_reports_none() {
    let rig = Rig::new("no-panel");
    let runner = rig.open().unwrap();

    let status = rig.ask(&runner, "status", None);

    assert!(status.get("panel").is_none(), "{status}");
}
