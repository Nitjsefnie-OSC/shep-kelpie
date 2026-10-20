use serde_json::json;

use super::*;
use crate::ports::{Checks, Cost, Usage};
use crate::runner::{StepReport, step};
use crate::skills::Step;
use crate::test::{Rig, Scripted, ticket};

mod board;
mod ticket;

// A running project with issue 7 in flight
fn with_issue_7(project: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    (rig, runner)
}

#[test]
fn issue_lists_read_as_prose() {
    assert_eq!(issue_list(&[ticket(7)]), "#7");
    assert_eq!(issue_list(&[ticket(7), ticket(9)]), "#7 and #9");
    assert_eq!(
        issue_list(&[ticket(7), ticket(9), ticket(12)]),
        "#7, #9 and #12"
    );
}

#[test]
fn a_new_project_has_nothing_in_flight() {
    let rig = Rig::new("koji");
    rig.retro_on();
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "status", None),
        json!({
            "project": "koji",
            "merging": "ask",
            "work_item": null,
            "work_items": [],
            "active_items": 1,
            "working": [],
            "waiting_for_slot": [],
            "parked": [],
            "pending_rulings": 2,
            "adopted": [],
            "order": [],
            "skipped": [],
            "rulings": [],
            "leases": [],
            "history": [],
            "pacer": { "enabled": true, "claude": { "reading": null, "holding": null } },
            // kelpie seeds a vendored skill for the worker's four and for
            // the retro this rig turned on, so `status` names no command
            // for triage, spec and reset: they run kelpie's own prompt.
            "skills": Step::ALL.map(|step| json!({
                "step": step.as_str(),
                "skill": (Step::WORKER.contains(&step) || step == Step::Retro)
                    .then(|| format!("/mattpocock:{}", step.default_skill())),
                "fallback": null,
            })),
            "pull_request_template": "none",
            "board": rig.paths().board,
            "build": crate::home::stamp_of(&std::env::current_exe().unwrap()).unwrap(),
        })
    );
}

#[test]
fn every_registered_action_is_answered_and_no_other() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    for action in ACTIONS.into_iter().filter(|a| {
        ![
            "rework", "adopt", "rule", "gate", "drop", "attach", "detach", "tell", "pm", "triage",
        ]
        .contains(a)
    }) {
        let params = (action == "add").then_some("7");
        assert_eq!(
            rig.ask(&runner, action, params)["project"],
            "koji",
            "{action}"
        );
    }
    assert_eq!(
        rig.ask(&runner, "rework", Some("71")),
        json!({ "error": "the work item for #7 is in flight" })
    );
    let no_pm = "the project has no project manager: name one in `agents.pm`, such as `pm`";
    let me = std::process::id();
    assert_eq!(
        rig.ask(&runner, "tell", Some("hold #4")),
        json!({ "error": no_pm })
    );
    assert_eq!(
        rig.ask(&runner, "pm", Some(&format!("attach {me}"))),
        json!({ "error": no_pm })
    );
    assert_eq!(
        rig.ask(&runner, "pm", Some(&format!("detach {me}"))),
        json!({ "error": no_pm })
    );
    assert_eq!(
        rig.ask(&runner, "pm", Some("attach 0")),
        json!({ "error": "`pm` takes `attach <pid>`, `attach <pid> <session pid>` or \
                          `detach <pid>`, not \"attach 0\"" })
    );
    assert_eq!(
        rig.ask(&runner, "adopt", Some("71")),
        json!({ "error": "cannot read pull request #71: gh failed: no pull request #71" })
    );
    assert_eq!(
        rig.ask(&runner, "rule", Some("71 yes")),
        json!({ "error": "ruling 71 is already answered, or was never raised" })
    );
    assert_eq!(
        rig.ask(&runner, "rule", None),
        json!({ "error": "`rule` takes `<id> yes`, `<id> no <note>`, `<id> rework <note>` or `<id> answer <text>`" })
    );
    assert_eq!(
        rig.ask(&runner, "merge", None),
        json!({ "error": "unknown action `merge`" })
    );
}

#[test]
fn a_trigger_with_params_is_refused_and_changes_nothing() {
    let rig = Rig::new("rotom");
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "drain", Some("now")),
        json!({ "error": "`drain` takes no params" })
    );
    assert_eq!(rig.ask(&runner, "status", None)["draining"], json!(null));
}

#[test]
fn a_second_add_while_one_is_in_flight_is_refused() {
    let (rig, runner) = with_issue_7("golbat");
    assert_eq!(
        rig.ask(&runner, "add", Some("8")),
        json!({ "error": "the work item for #7 is in flight" })
    );
}

#[test]
fn adding_an_issue_the_forge_cannot_show_changes_nothing() {
    let rig = Rig::new("reactmap");
    rig.forge.remove_issue(9);
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("9")),
        json!({ "error": "cannot read the issue: gh failed: no issue #9" })
    );
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn gate_sends_a_work_item_saved_before_the_gate_to_its_merge_ruling() {
    let (rig, runner, head) = Rig::with_pull_request("webapp");
    drop(runner);
    let state = rig.paths().state;
    let text = std::fs::read_to_string(&state).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    let item = saved["work_items"][0].as_object_mut().unwrap();
    item.remove("phase");
    item.remove("red_head");
    std::fs::write(&state, saved.to_string()).unwrap();

    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    assert_eq!(step(&runner).unwrap(), None, "left alone until asked");
    let status = rig.ask(&runner, "gate", None);
    assert_eq!(
        status["work_item"]["phase"],
        json!({ "state": "ci", "head": null, "since": Rig::EPOCH })
    );
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id, .. })
     if id == ticket(71)));
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn gate_refuses_anything_but_an_ended_turn_with_a_known_pull_request() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    let refused = |runner: &Mutex<Runner>, error: &str| {
        assert_eq!(rig.ask(runner, "gate", None), json!({ "error": error }));
    };
    refused(&runner, "no work item is in flight");
    rig.ask(&runner, "add", Some("7"));
    refused(&runner, "the worker's turn on #7 is due, not ended");
    assert_eq!(
        rig.ask(&runner, "gate", Some("7")),
        json!({ "error": "the worker's turn on #7 is due, not ended" })
    );
    assert_eq!(
        rig.ask(&runner, "gate", Some("8")),
        json!({ "error": "no work item for #8 is open" })
    );
    assert_eq!(
        rig.ask(&runner, "gate", Some("#7")),
        json!({ "error": "\"#7\" is not a ticket id" })
    );

    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(&runner).unwrap();
    refused(&runner, "no pull request is known for #7");

    let (rig, runner, _) = Rig::with_pull_request("rotom");
    assert_eq!(
        rig.ask(&runner, "gate", None),
        json!({ "error": "the work item for #7 is already in the gate" })
    );
}
