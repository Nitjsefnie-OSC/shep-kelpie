//! A ruling raised while the runner drains: the panel waits for a runner
//! that can start calls, and a ruling that waits too long goes to the
//! maintainer with the reason it has no votes (shep-pm/shep-kelpie#711)

use std::sync::Mutex;

use super::{name_panel, ruling_comment, script_all, seen, settle, state_of};
use crate::ports::{Checks, Role};
use crate::runner::panel::HOLD;
use crate::runner::{Runner, StepReport, step};
use crate::test::Rig;

/// A merge ruling raised on pull request 71 by a runner that drains
fn raised_in_a_drain(project: &str) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = Rig::with_pull_request_set(project, |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        name_panel(rig, false);
    });
    rig.forge.set_checks(&head, Checks::Passed);
    rig.ask(&runner, "drain", None);
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id: 71, .. })
    ));
    (rig, runner)
}

#[test]
fn a_ruling_raised_in_a_drain_waits_and_is_voted_on_by_the_runner_that_restarts() {
    let (rig, runner) = raised_in_a_drain("tangela");
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );
    assert!(
        seen(&rig, Role::Advisor).is_empty(),
        "a drain starts no call"
    );
    assert_eq!(state_of(&rig)["rulings"][0]["unposted"], true);
    drop(runner);
    script_all(&rig, false);

    let runner = rig.open().unwrap();
    let comment = settle(&rig, &runner);

    assert!(comment.contains("**Panel**"), "{comment}");
    let ruling = &state_of(&rig)["rulings"][0];
    assert_eq!(
        ruling["panel"].as_array().map(Vec::len),
        Some(3),
        "{ruling}"
    );
    assert_eq!(ruling.get("skipped"), None);
}

#[test]
fn a_ruling_raised_in_a_drain_is_voted_on_once_the_runner_undrains() {
    let (rig, runner) = raised_in_a_drain("gloom");
    for _ in 0..4 {
        step(&runner).unwrap();
    }
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );
    script_all(&rig, false);

    rig.ask(&runner, "undrain", None);
    let comment = settle(&rig, &runner);

    assert!(comment.contains("**Panel**"), "{comment}");
}

#[test]
fn a_ruling_that_waited_a_hold_for_a_drain_to_end_goes_to_the_maintainer_and_says_why() {
    let (rig, runner) = raised_in_a_drain("vileplume");
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );

    rig.clock.advance(HOLD + 1);
    let comment = settle(&rig, &runner);

    assert!(!comment.contains("**Panel**"), "{comment}");
    assert_eq!(state_of(&rig)["rulings"][0]["skipped"], "draining");
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("ruling 71: the panel skips it, since the runner drained"),
        "{notes}"
    );
    rig.ask(&runner, "rule", Some("71 yes"));
    let line = std::fs::read_to_string(rig.paths().rulings).unwrap();
    let record: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(record["skipped"], "draining", "{record}");
    assert_eq!(
        rig.ask(&runner, "status", None)["panel"]["skipped"],
        serde_json::json!({ "draining": 1, "posted": 0 })
    );
}

#[test]
fn a_ruling_whose_alert_went_out_before_the_panel_took_it_up_is_posted_and_says_why() {
    let (rig, runner) = raised_in_a_drain("lickitung");
    drop(runner);
    let mut state = state_of(&rig);
    state["rulings"][0]["alerted"] = true.into();
    std::fs::write(rig.paths().state, state.to_string()).unwrap();

    let runner = rig.open().unwrap();
    let comment = settle(&rig, &runner);

    assert!(!comment.contains("**Panel**"), "{comment}");
    assert!(
        seen(&rig, Role::Advisor).is_empty(),
        "the panel was not asked"
    );
    let ruling = &state_of(&rig)["rulings"][0];
    assert_eq!(ruling["skipped"], "posted", "{ruling}");
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("ruling 71: the panel skips it, since its comment had gone out"),
        "{notes}"
    );
}
