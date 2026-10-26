use std::sync::Mutex;

use super::super::CHECKS_SETTLE;
use super::super::tests::ruling_report;
use crate::ports::Checks;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, ticket};
use serde_json::json;

const CAP: u64 = 60 * 60;

// The pull request is open and CI blocks, and the forge reports no check on its
// head. `main` holds no workflow that runs on a pull request.
fn without_workflow() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) =
        Rig::with_pull_request_set("shep", |rig| rig.without_pull_request_ci());
    rig.forge.set_checks(&head, Checks::None);
    (rig, runner, head)
}

// The same, with the workflow `main` holds running on a pull request.
fn with_workflow() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    rig.forge.set_checks(&head, Checks::None);
    (rig, runner, head)
}

fn parked_on_no_checks(rig: &Rig, runner: &Mutex<Runner>) -> String {
    assert_eq!(step(runner).unwrap(), None, "the wait starts");
    rig.clock.advance(CAP - 1);
    assert_eq!(step(runner).unwrap(), None, "within the cap");
    rig.clock.advance(1);
    let (id, question) = ruling_report(step(runner).unwrap());
    assert_eq!(id, ticket(71));
    question
}

#[test]
fn with_no_pull_request_workflow_a_head_without_checks_goes_on_as_skipped() {
    let (rig, runner, head) = without_workflow();

    assert_eq!(step(&runner).unwrap(), None, "the checks settle first");
    rig.clock.advance(CHECKS_SETTLE);
    let (id, question) = ruling_report(step(&runner).unwrap());

    assert_eq!(id, ticket(71));
    assert!(question.starts_with("Merge pull request #71"), "{question}");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["rulings"][0]["kind"],
        json!({ "kind": "merge", "head": head })
    );
    assert_eq!(status["rulings"].as_array().unwrap().len(), 1);
}

#[test]
fn with_no_pull_request_workflow_a_head_without_checks_merges_under_auto() {
    let (rig, runner, head) = Rig::with_pull_request_set("shep", |rig| {
        rig.merge_auto();
        rig.without_pull_request_ci();
    });
    rig.forge.set_checks(&head, Checks::None);

    let first = rig.verdict(&runner);
    assert!(
        matches!(first, Some(StepReport::MarkedReady { .. })),
        "{first:?}"
    );
    let second = rig.verdict(&runner);
    assert!(
        matches!(second, Some(StepReport::Finished { merged: true, .. })),
        "{second:?}"
    );
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

#[test]
fn with_a_pull_request_workflow_a_head_without_checks_waits_for_a_ruling_past_the_cap() {
    let (rig, runner, head) = with_workflow();

    let question = parked_on_no_checks(&rig, &runner);

    assert_eq!(
        question,
        format!(
            "No check has appeared on pull request #71 at {} in the `ci.queued_minutes` kelpie \
             waits, though a workflow runs on pull requests. `shep kelpie rule 71 yes` passes \
             this head as skipped and kelpie goes on, and `shep kelpie rule 71 no <note>` \
             waits another `ci.queued_minutes`.",
            &head[..7]
        )
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["rulings"][0]["kind"],
        json!({ "kind": "stuck", "reason": "no-checks", "head": head })
    );
    assert_eq!(status["work_item"]["phase"]["state"], "ruling");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { id: ticket(71) })
    );
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "one ruling, however long it waits"
    );
    assert_eq!(
        rig.ask(&runner, "status", None)["rulings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_yes_on_no_checks_passes_the_head_as_skipped() {
    let (rig, runner, _) = with_workflow();
    parked_on_no_checks(&rig, &runner);

    rig.ask(&runner, "rule", Some("71 yes"));

    let (_, question) = ruling_report(rig.verdict(&runner));
    assert!(question.starts_with("Merge pull request #71"), "{question}");
}

#[test]
fn a_no_on_no_checks_waits_another_cap() {
    let (rig, runner, _) = with_workflow();
    parked_on_no_checks(&rig, &runner);

    rig.ask(&runner, "rule", Some("71 no it is still starting"));

    assert_eq!(step(&runner).unwrap(), None, "it waits again");
    rig.clock.advance(CAP - 1);
    assert_eq!(step(&runner).unwrap(), None, "the cap starts from the no");
    rig.clock.advance(1);
    let (_, question) = ruling_report(step(&runner).unwrap());
    assert!(question.starts_with("No check has appeared"), "{question}");
}

#[test]
fn a_check_that_appears_during_the_wait_is_read_as_usual() {
    let (rig, runner, head) = with_workflow();
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(CAP - 1);
    assert_eq!(step(&runner).unwrap(), None);

    rig.forge.set_checks(&head, Checks::Passed);
    let report = step(&runner).unwrap();

    assert!(
        matches!(report, Some(StepReport::Ruling { .. })),
        "{report:?}"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"][0]["kind"]["kind"], "merge");
}

#[test]
fn a_new_push_starts_the_no_checks_wait_over() {
    let (rig, runner, _) = with_workflow();
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(CAP - 1);
    assert_eq!(step(&runner).unwrap(), None, "within the cap");

    let pushed = rig.push_by_hand("kelpie/7", "late.txt");
    rig.forge.set_checks(&pushed, Checks::None);
    let (id, _) = ruling_report(step(&runner).unwrap());
    rig.ask(&runner, "rule", Some(&format!("{id} yes")));
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "the new head starts its own wait"
    );
    rig.clock.advance(CAP - 1);
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "the old head's cap does not carry over"
    );
    rig.clock.advance(1);
    let (_, question) = ruling_report(step(&runner).unwrap());
    assert!(question.starts_with("No check has appeared"), "{question}");
    assert!(question.contains(&pushed[..7]), "{question}");
}
