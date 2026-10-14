//! A work item past its spending cap

use super::*;

const DOLLAR: u64 = 1_000_000_000;

// Issue 7 in flight under `budget.per_item_usd = 5`
fn with_a_five_dollar_cap(project: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.edit_settings(|s| s.replace("per_item_usd = 10", "per_item_usd = 5"));
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    (rig, runner)
}

#[test]
fn a_work_item_past_its_cap_raises_a_ruling_before_the_worker_is_called_again() {
    let (rig, runner) = with_a_five_dollar_cap("acme");
    // The first turn costs $6 and stops short, which would send the worker back.
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(6 * DOLLAR))]);
    step(&runner).unwrap();

    let Some(StepReport::Failed {
        issue: 7,
        pull_request: None,
        id: 7,
        question,
        ..
    }) = step(&runner).unwrap()
    else {
        panic!("the worker was not parked");
    };
    assert_eq!(
        question,
        "The work on issue #7 has cost $6.00 against its cap of $5.00: the worker $6.00 and \
         the reviewers $0.00, so the worker's next turn has not started. \
         `shep kelpie rule 7 yes` raises the cap to $10.00 and the worker carries on, and \
         `shep kelpie rule 7 no <note>` sends the worker your note and raises the cap the same."
    );
    assert_eq!(rig.claude.calls().len(), 1, "no call after the cap");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["rulings"][0]["kind"],
        json!({
            "kind": "stuck",
            "reason": "over-budget",
            "phase": { "state": "implement" },
            "spent": 6 * DOLLAR,
            "cap": 5 * DOLLAR,
            "by_role": { "worker": 6 * DOLLAR, "reviewer": 0 },
        })
    );
}

#[test]
fn status_shows_what_a_work_item_has_spent_against_its_cap() {
    let (rig, runner) = with_a_five_dollar_cap("acme");
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(3 * DOLLAR))]);
    step(&runner).unwrap();
    let budget = &rig.ask(&runner, "status", None)["work_item"]["budget"];
    assert_eq!(budget, &json!({ "spent_usd": 3.0, "cap_usd": 5.0 }));
}

#[test]
fn a_work_item_under_its_cap_is_left_alone() {
    let (rig, runner) = with_a_five_dollar_cap("acme");
    rig.claude.script([
        Scripted::Reply(usage(1), Cost(5 * DOLLAR)),
        Scripted::Reply(usage(1), Cost(5 * DOLLAR)),
    ]);
    step(&runner).unwrap();
    step(&runner).unwrap();
    let [_, again] = rig.claude.calls().try_into().unwrap();
    assert_eq!(again.prompt, SENT_BACK, "exactly the cap is not over it");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
}
