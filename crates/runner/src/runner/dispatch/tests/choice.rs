// `agents.implementer_choice = "order"`, and an `agent:` label naming an
// implementer the project does not list

use serde_json::json;
use std::sync::Mutex;

use super::*;

// An implementer on a local model, and a second on another local model
const CODER: &str = "---\nrole: implementer\nharness: stand-in\nmodel: qwen3-coder\n\
                      effort: low\nusage: none\n---\n";
const CODER2: &str = "---\nrole: implementer\nharness: stand-in\nmodel: qwen3-coder2\n\
                       effort: low\nusage: none\n---\n";

// A project listing `listed`, each on a local model of its own, with
// `agents.implementer_choice = "order"`
fn in_order(project: &str, listed: &[&str]) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.write_agent("coder", CODER);
    rig.write_agent("coder2", CODER2);
    rig.implementers(listed);
    rig.implementers_in_order();
    let runner = rig.open().unwrap();
    (rig, runner)
}

// Whether `report` dispatched issue `issue` on agent `agent`
fn dispatched_on(report: Option<StepReport>, issue: u64, agent: &str) -> bool {
    matches!(report, Some(StepReport::Dispatched { issue: got, agent: picked, .. })
        if got == ticket(issue) && picked.as_str() == agent)
}

#[test]
fn implementer_choice_order_opens_an_unlabelled_issue_on_the_first_listed_with_no_call() {
    let (rig, runner) = in_order("acme", &["coder", "coder2"]);
    rig.forge.list_ready(6, false);
    assert!(
        dispatched_on(step(&runner).unwrap(), 6, "coder"),
        "the first listed implementer takes it"
    );
    assert_eq!(rig.claude.calls(), [], "no issue writer call");
    assert!(
        !(rig.forge.issue_labels(6))
            .iter()
            .any(|l| l.starts_with("agent:")),
        "no label is written onto the issue: {:?}",
        rig.forge.issue_labels(6)
    );

    rig.claude.script([Scripted::Say("done")]);
    step(&runner).unwrap();
    let call = &rig.claude.calls()[0];
    assert_eq!(
        (call.role, call.model.as_str()),
        (Role::Worker, "qwen3-coder")
    );
}

#[test]
fn implementer_choice_order_leaves_an_issues_own_label_in_charge() {
    let (rig, runner) = in_order("acme", &["coder", "coder2"]);
    rig.forge.list_ready(6, false);
    rig.forge.label(6, "agent:coder2");
    assert!(dispatched_on(step(&runner).unwrap(), 6, "coder2"));
    assert_eq!(rig.claude.calls(), [], "no issue writer call");

    rig.claude.script([Scripted::Say("done")]);
    step(&runner).unwrap();
    assert_eq!(rig.claude.calls()[0].model, "qwen3-coder2");
}

#[test]
fn add_takes_a_label_naming_an_unlisted_implementer_and_refuses_one_naming_none() {
    let rig = Rig::new("xilriws");
    rig.write_agent("coder", CODER);
    rig.edit_settings(|s| s.replace("active_items = 1", "active_items = 2"));
    let runner = rig.open().unwrap();
    let refused = |issue: u64, label: &str| {
        rig.forge.label(issue, label);
        assert_eq!(
            rig.ask(&runner, "add", Some(&issue.to_string())),
            json!({
                "error": format!(
                    "label `{label}` names no implementer: kelpie's agent files with \
                     `role: implementer` are coder, opus-high, sonnet-high"
                )
            }),
            "{label}"
        );
    };
    refused(9, "agent:defect-hunter");
    refused(10, "agent:haiku-low");

    let agent_of = |issue: u64| {
        let status = rig.ask(&runner, "status", None);
        let items = status["work_items"].as_array().unwrap();
        let shown = issue.to_string();
        let found = items
            .iter()
            .find(|i| i["issue"] == json!(shown))
            .expect("open");
        found["agent"].clone()
    };
    rig.forge.label(8, "agent:coder");
    rig.ask(&runner, "add", Some("8"));
    assert_eq!(agent_of(8), json!("coder"), "the label was the choice");

    rig.forge.label(11, "worker:haiku-low");
    rig.ask(&runner, "add", Some("11"));
    assert_eq!(agent_of(11), json!("sonnet-high"));
    let notes = runner.lock().unwrap().take_notes();
    assert_eq!(
        notes,
        [
            "issue #11 is labelled `worker:haiku-low`, which kelpie no longer reads, so it \
          runs on the default implementer, sonnet-high: an `agent:<name>` label picks \
          another implementer"
        ]
    );
}
