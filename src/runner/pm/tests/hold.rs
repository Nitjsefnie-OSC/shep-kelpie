//! The held wake reasons wait a window, so several of them go in one call

use std::sync::Mutex;

use crate::ports::AgentError;
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted};

use super::{answered, pm_seen, step_until, with_pm};
use crate::runner::pm::HOLD_FOR;

const NOTHING: &str = "{\"pick\": null}";

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
    rig.claude.script([Scripted::Fail(failed()), Scripted::Say(NOTHING)]);
    rig.ask(&runner, "add", Some("9"));
    step_until(&runner, |r| matches!(r, StepReport::Failed { issue: 9, .. }));
    (rig, runner)
}

#[test]
fn two_stuck_items_a_minute_apart_wake_it_once_with_both_reasons() {
    let (rig, runner) = two_items_a_minute_apart("acme");

    assert!(
        pm_seen(&rig).is_empty(),
        "a stuck reason woke it before its window ended"
    );

    rig.clock.advance(HOLD_FOR);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, dropped) = answered(Some(report));
    assert_eq!(woke_for.len(), 2, "{woke_for:?}");
    assert!(woke_for[0].starts_with("#7 is stuck: "), "{woke_for:?}");
    assert!(woke_for[1].starts_with("#9 is stuck: "), "{woke_for:?}");
    assert_eq!((acted, dropped), (vec![], vec![]));
    assert_eq!(pm_seen(&rig).len(), 1, "one call for both reasons");
}
