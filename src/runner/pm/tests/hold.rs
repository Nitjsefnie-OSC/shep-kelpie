//! The held wake reasons wait a window, so several of them go in one call

use std::sync::Mutex;

use serde_json::json;

use crate::ports::{AgentError, CallActivity, Timestamp};
use crate::runner::flight::advance;
use crate::runner::{Pass, Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

use super::{PICK_12, answered, dispatched, pm_seen, step_until, with_pm};
use crate::runner::pm::HOLD_FOR;

const NOTHING: &str = "{\"pick\": null}";

const ASKS: &str = "I added the flag.\n\n<kelpie-question>\nShould it be `--dry-run` or \
                    `--check`?\n\nBoth appear in the docs.\n</kelpie-question>\n";

/// What a wake for a free slot says
const PICK_REASON: &str = "a slot is free and 2 ready issues could fill it: pick one, or none \
                           to start nothing now";

/// `woke_for` as the test's list of reasons
fn lines(woke_for: &[String]) -> Vec<&str> {
    woke_for.iter().map(String::as_str).collect()
}

/// What a wake says of `issue`, parked on ruling `ruling` for its failed turn
fn stuck_reason(issue: u64, ruling: u64) -> String {
    format!(
        "#{issue} is stuck: its worker's turn parked: the call failed once, the model host \
         stayed unreachable for 30 minutes, or the turn stopped short twice, and ruling \
         {ruling} asks the maintainer"
    )
}

/// A project with a project manager, #7's worker, and its turn about to fail
fn one_item(project: &str) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = with_pm(project);
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Fail(failed())]);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Failed { .. })
    ));
    (rig, runner)
}

fn failed() -> AgentError {
    AgentError::Failed(Harness::ClaudeCode, "it crashed".into())
}

/// #7's turn fails, and a minute later #9's does, each parking its item on a
/// stuck ruling
fn two_items_a_minute_apart(project: &str) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = one_item(project);
    rig.clock.advance(60);
    rig.claude.script([Scripted::Fail(failed())]);
    rig.ask(&runner, "add", Some("9"));
    step_until(&runner, |r| {
        matches!(r, StepReport::Failed { issue: 9, .. })
    });
    (rig, runner)
}

#[test]
fn two_stuck_items_a_minute_apart_wake_it_once_with_both_reasons() {
    let (rig, runner) = two_items_a_minute_apart("acme");
    rig.claude.script([Scripted::Say(NOTHING)]);

    assert!(
        pm_seen(&rig).is_empty(),
        "a stuck reason woke it before its window ended"
    );

    rig.clock.advance(HOLD_FOR);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, dropped) = answered(Some(report));
    assert_eq!(
        woke_for,
        [stuck_reason(7, 7), stuck_reason(9, 9)],
        "both stuck items went in one wake"
    );
    assert_eq!((acted, dropped), (vec![], vec![]));
    assert_eq!(pm_seen(&rig).len(), 1, "one call for both reasons");
}

#[test]
fn a_tell_inside_the_window_wakes_it_at_once_with_the_held_reasons() {
    let (rig, runner) = two_items_a_minute_apart("eevee");
    rig.claude.script([Scripted::Say(NOTHING)]);

    rig.ask(&runner, "tell", Some("hold #12 until the release"));
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, ..) = answered(Some(report));
    assert_eq!(
        lines(&woke_for),
        [
            stuck_reason(7, 7),
            stuck_reason(9, 9),
            "the maintainer told you something".to_owned()
        ],
        "the tell went at once, carrying what was held"
    );
    let [wake] = pm_seen(&rig).try_into().unwrap();
    assert!(
        wake.call
            .prompt
            .contains("  > hold #12 until the release\n"),
        "{}",
        wake.call.prompt
    );
}

#[test]
fn a_workers_question_inside_the_window_wakes_it_at_once_with_the_held_reasons() {
    let (rig, runner) = one_item("chelone");
    rig.ask(&runner, "add", Some("9"));
    rig.claude
        .script([Scripted::Say(ASKS), Scripted::Say(NOTHING)]);
    step_until(&runner, |r| matches!(r, StepReport::Asked { .. }));

    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, dropped) = answered(Some(report));
    assert_eq!(
        lines(&woke_for),
        [
            stuck_reason(7, 7),
            "#9's worker asks, on ruling 9:\n\n> Should it be `--dry-run` or `--check`?\n>\n> Both appear in the docs.".to_owned()
        ],
        "the question went at once, carrying what was held"
    );
    assert_eq!((acted, dropped), (vec![], vec![]));
    assert_eq!(pm_seen(&rig).len(), 1, "one call for both reasons");
}

#[test]
fn a_free_slot_wakes_it_at_once() {
    let (rig, runner) = one_item("koji");
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude.script([Scripted::Say(PICK_12)]);

    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, _) = answered(Some(report));
    assert_eq!(
        lines(&woke_for),
        [stuck_reason(7, 7), PICK_REASON.to_owned()],
        "the free slot went at once, carrying what was held"
    );
    assert_eq!(acted, ["picked #12".to_owned()]);
    assert_eq!(dispatched(step(&runner).unwrap()), 12);
}

#[test]
fn a_held_reason_that_stops_being_true_inside_the_window_wakes_nothing() {
    let (rig, runner) = with_pm("acme");
    rig.ask(&runner, "add", Some("7"));
    let work = Hold::default();
    rig.claude
        .script([Scripted::HoldThenSay(work.clone(), "done")]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(work.entered(CALL_PATIENCE), "the turn never began");
    rig.claude.set_activity(CallActivity::Nothing);
    rig.clock.advance(11 * 60);
    // The board tells the worker idle on this pass, which opens its window.
    assert_eq!(step(&runner).unwrap(), None, "the idle line is no report");

    // The worker shows output again before the window ends.
    rig.claude
        .set_activity(CallActivity::At(Timestamp(Rig::EPOCH + 11 * 60)));
    rig.clock.advance(HOLD_FOR);
    assert!(matches!(advance(&runner).unwrap(), Pass::Idle));
    assert!(
        pm_seen(&rig).is_empty(),
        "the worker is active again, so its reason is dropped and nothing woke"
    );

    work.release();
    assert!(work.answered(CALL_PATIENCE), "the turn never answered");
}

#[test]
fn a_held_reason_given_up_while_it_is_down_is_sent_when_it_is_back() {
    let (rig, runner) = one_item("eevee");
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude
        .script([Scripted::Fail(down), Scripted::Say(NOTHING)]);

    rig.clock.advance(HOLD_FOR);
    let failed = step_until(&runner, |r| matches!(r, StepReport::PmFailed { .. }));
    assert!(matches!(failed, StepReport::PmFailed { .. }), "{failed:?}");
    assert_eq!(
        rig.ask(&runner, "status", None)["pm"]["down_until"],
        Rig::EPOCH + HOLD_FOR + 600,
        "its call failed, so it is passed over for 10 minutes"
    );

    rig.clock.advance(10 * 60);
    // Its reason is queued again on this pass, which opens its window.
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "nothing woke while it was down"
    );
    rig.clock.advance(HOLD_FOR);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, _, _) = answered(Some(report));
    assert_eq!(
        lines(&woke_for),
        [stuck_reason(7, 7)],
        "the reason it gave up while it was down reaches it once it is back"
    );
    assert_eq!(pm_seen(&rig).len(), 2, "the failed wake and this one");
}

#[test]
fn one_answer_settles_two_stuck_items() {
    let (rig, runner) = two_items_a_minute_apart("golbat");
    rig.claude.script([Scripted::Say(
        "{\"pick\": null, \"unstick\": [{\"item\": 7, \"action\": \"retry\", \"why\": \"an          outage\"}, {\"item\": 9, \"action\": \"retry\", \"why\": \"the same outage\"}]}",
    )]);

    rig.clock.advance(HOLD_FOR);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, dropped) = answered(Some(report));
    assert_eq!(lines(&woke_for), [stuck_reason(7, 7), stuck_reason(9, 9)]);
    assert_eq!(acted, ["retried #7".to_owned(), "retried #9".to_owned()]);
    assert_eq!(dropped, Vec::<String>::new());
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
}
