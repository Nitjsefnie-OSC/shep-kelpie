//! What the panel's votes leave behind: a ledger line each

use serde_json::{Value, json};

use super::{SHADOW, leak, pm_says, ruling, says, voted};
use crate::ports::{Cost, Role};
use crate::settings::Merging;
use crate::test::ticket;
use crate::test::{CALL_PATIENCE, Rig, Scripted};
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
                "ruling": "71", "ruling_kind": "merge", "voter": voter,
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

#[test]
fn a_wake_that_failed_casts_no_votes_for_its_retry_to_repeat() {
    let (rig, runner) = ruling("failed-wake", "feat: add it", Merging::Auto, SHADOW);
    for (role, who) in [
        (Role::Worker, "WORKER"),
        (Role::Researcher, "RESEARCHER"),
        (Role::Advisor, "ADVISOR"),
    ] {
        rig.claude
            .script_for(role, [Scripted::Say(leak(says(who, 'A', false)))]);
    }
    rig.claude
        .script_for(Role::Pm, [Scripted::Say("I cannot say.")]);

    let deadline = std::time::Instant::now() + CALL_PATIENCE;
    let wake = |lines: &[Value]| {
        lines
            .iter()
            .any(|l| l["role"] == "pm" && l["kind"] == "wake")
    };
    while std::time::Instant::now() < deadline && !wake(&ledger(&rig)) {
        crate::runner::step(&runner).unwrap();
        std::thread::yield_now();
    }

    let lines = ledger(&rig);
    assert!(wake(&lines), "the wake never ended: {lines:#?}");
    let pm_votes = lines.iter().filter(|l| l["votes"][0]["voter"] == "pm");
    assert_eq!(pm_votes.count(), 0, "{lines:#?}");
}

#[test]
fn a_closed_rulings_plan_is_kept_for_a_wake_that_outlasts_it_and_the_oldest_go() {
    use crate::panel::work_of;
    use crate::runner::panel::{Desk, Plan};
    use crate::settings::RulingName;
    let mut desk = Desk::default();
    let plan = Plan::shadow(RulingName::Merge, work_of(Some("fix: a bug")));
    desk.retire(ticket(1), plan.clone());
    let mark = desk
        .plan(ticket(1))
        .expect("kept")
        .mark(ticket(1), crate::panel::Voter::Pm, None);
    assert_eq!(
        (mark.ruling_kind, mark.work.as_str()),
        (RulingName::Merge, "fix")
    );
    for id in 2..=40 {
        desk.retire(ticket(id), plan.clone());
    }
    assert!(desk.plan(ticket(1)).is_none(), "the oldest was let go");
    assert!(desk.plan(ticket(40)).is_some());
}
