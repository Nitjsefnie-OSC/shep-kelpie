//! A project manager's pick through a board that changes before dispatch
//! takes it

use std::sync::Mutex;

use serde_json::{Value, json};

use super::{PICK_12, answered, pm_seen, step_until, with_pm};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted, ticket};

// A project with #9 and #12 ready, whose project manager picked #12 and has
// not been stepped past it.
fn picked_12() -> (Rig, Mutex<Runner>) {
    let (rig, runner) = with_pm("koji");
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude.script([Scripted::Say(PICK_12)]);
    let (_, acted, _) = answered(step(&runner).unwrap());
    assert_eq!(acted, ["picked #12"]);
    (rig, runner)
}

fn dispatched_12(runner: &Mutex<Runner>) {
    let report = step_until(runner, |r| matches!(r, StepReport::Dispatched { .. }));
    assert!(matches!(report, StepReport::Dispatched { issue, .. } if issue == ticket(12)));
}

// The steps logged in the middle of a pass that name a dropped pick.
fn pick_dropped_steps(runner: &Mutex<Runner>) -> Vec<Value> {
    let started = runner.lock().unwrap().take_started();
    let all = started.iter().map(|s| serde_json::to_value(s).unwrap());
    all.filter(|s| s["step"] == "pick-dropped").collect()
}

#[test]
fn a_pick_survives_an_unrelated_item_opening_and_is_dispatched() {
    let (rig, runner) = picked_12();

    rig.ask(&runner, "add", Some("7"));

    dispatched_12(&runner);
    assert_eq!(pick_dropped_steps(&runner), Vec::<Value>::new());
}

#[test]
fn a_pick_survives_an_unrelated_item_closing_and_is_dispatched() {
    let (rig, runner) = with_pm("koji");
    rig.ask(&runner, "add", Some("7"));
    step_until(&runner, |r| matches!(r, StepReport::Failed { .. }));
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude.script([Scripted::Say(PICK_12)]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    assert!(matches!(report, StepReport::PmAnswered { acted, .. } if acted == ["picked #12"]));

    rig.ask(&runner, "drop", Some("7"));

    dispatched_12(&runner);
    assert_eq!(pick_dropped_steps(&runner), Vec::<Value>::new());
}

#[test]
fn a_pick_the_board_can_no_longer_take_leaves_a_step_and_is_told_to_the_next_wake() {
    let (rig, runner) = picked_12();

    // The maintainer opens the picked issue before dispatch takes it.
    rig.ask(&runner, "add", Some("12"));
    step_until(&runner, |r| matches!(r, StepReport::Dispatched { .. }));
    // The worker's turn has no scripted reply, which the project manager's
    // next wake must not take for its own.
    step_until(
        &runner,
        |r| matches!(r, StepReport::Failed { issue, .. } if *issue == ticket(9)),
    );

    assert_eq!(
        pick_dropped_steps(&runner),
        [json!({
            "step": "pick-dropped",
            "issue": "12",
            "why": "it is not an issue the board can take now",
        })]
    );

    rig.claude.script([Scripted::Say(PICK_12)]);
    rig.ask(&runner, "tell", Some("anything"));
    step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let wakes = pm_seen(&rig);
    let prompt = &wakes.last().unwrap().call.prompt;
    assert!(
        prompt.contains(
            "- kelpie dropped your pick #12 and the board rule picked: it is not an issue \
             the board can take now"
        ),
        "{prompt}"
    );
}
