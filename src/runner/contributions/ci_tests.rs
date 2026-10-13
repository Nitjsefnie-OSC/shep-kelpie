//! Approving a trusted head's held CI runs and following them, through the
//! runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use super::ruling_tests::{flagged, rule};
use super::security_tests::{commit_on, contribute, contribution};
use super::tests::{picking, poll};
use crate::ports::{AgentError, CLEAN, Checks};
use crate::runner::{CHECKS_SETTLE, Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted};

// What the security reviewer finds in a head it flags
const FOUND: &str = "HIGH|build.rs:3|downloads and runs a script|it runs at build time";

fn write(folder: &std::path::Path, file: &str, text: &str) {
    std::fs::write(folder.join(file), text).unwrap();
}

// A project that blocks on CI, with `pick = "all"`
fn blocking() -> (Rig, Mutex<Runner>) {
    let (rig, runner) = picking("all");
    rig.ci_on_now(&runner);
    (rig, runner)
}

// The followed contribution `number`, as `status` lists it
fn followed(rig: &Rig, runner: &Mutex<Runner>, number: u64) -> Value {
    let status = rig.ask(runner, "status", None);
    let listed = status["contributions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let found = listed.into_iter().find(|c| c["pull_request"] == number);
    found.expect("a followed contribution")
}

#[test]
fn a_passed_heads_pending_runs_are_approved_once() {
    let (rig, runner) = picking("all");
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(contribution(&rig, &runner)["passed"], head);

    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.forge.approvals(), [head]);
}

#[test]
fn a_flagged_unread_or_old_head_gets_no_approval() {
    // Flagged: the review found something, and nothing runs until a yes.
    let (rig, runner) = picking("all");
    rig.claude.script([Scripted::Text(FOUND)]);
    contribute(&rig, 30, |h| write(h, "build.rs", "fn main() {}\n"));
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), Vec::<String>::new());

    // Unread, and old: the head that passed is not the one pushed since.
    let (rig, runner) = picking("all");
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude
        .script([Scripted::Text(CLEAN), Scripted::Fail(down)]);
    let first = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), vec![first.clone()]);
    let second = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    poll(&rig, &runner);

    let held = contribution(&rig, &runner);
    assert_eq!(held["head"], second);
    assert_eq!(held["passed"], first);
    assert_eq!(held.get("security"), None, "{held}");
    assert_eq!(rig.forge.approvals(), [first]);
}

#[test]
fn a_head_trusted_by_a_yes_is_approved() {
    let (rig, runner, head) = flagged();
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), Vec::<String>::new());

    let answered = rule(&rig, &runner, &format!("30 yes {}", &head[..7]));
    assert_eq!(answered.get("error"), None, "{answered}");
    poll(&rig, &runner);

    assert_eq!(rig.forge.approvals(), [head]);
}

#[test]
fn a_failed_approval_is_tried_again_at_the_next_poll() {
    let (rig, runner) = blocking();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    rig.forge.set_approvals_refused(true);
    assert_eq!(step(&runner).unwrap(), None);

    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), Vec::<String>::new());
    assert_eq!(contribution(&rig, &runner).get("ci"), None);

    rig.forge.set_approvals_refused(false);
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(rig.forge.approvals(), [head]);
    assert_eq!(contribution(&rig, &runner)["ci"]["state"], "waiting");
}

#[test]
fn ci_is_waiting_until_the_rollup_settles_then_green_or_red_with_the_failed_checks() {
    let (rig, runner) = blocking();
    rig.claude
        .script([Scripted::Text(CLEAN), Scripted::Text(CLEAN)]);
    let green = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    let red = contribute(&rig, 31, |h| write(h, "two.txt", "two\n"));
    rig.forge.set_checks(&green, Checks::Passed);
    rig.forge
        .set_checks(&red, Checks::Failed(vec!["test".into(), "lint".into()]));
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals().len(), 2);

    // The rollup is on the forge, but checks register one at a time.
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "waiting");
    assert_eq!(followed(&rig, &runner, 31)["ci"]["state"], "waiting");

    rig.clock.advance(CHECKS_SETTLE);
    poll(&rig, &runner);

    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");
    assert_eq!(
        followed(&rig, &runner, 31)["ci"]["state"],
        serde_json::json!({"red": ["test", "lint"]})
    );
}

#[test]
fn ci_stays_waiting_while_a_check_is_pending_or_none_has_reported() {
    let (rig, runner) = blocking();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    rig.clock.advance(CHECKS_SETTLE);

    for checks in [Checks::None, Checks::Pending] {
        rig.forge.set_checks(&head, checks);
        poll(&rig, &runner);
        assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "waiting");
    }

    rig.forge.set_checks(&head, Checks::Passed);
    poll(&rig, &runner);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");
}

#[test]
fn a_rollup_of_only_skipped_runs_is_green() {
    let (rig, runner) = blocking();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    rig.forge.set_checks(&head, Checks::Skipped);
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    rig.clock.advance(CHECKS_SETTLE);

    poll(&rig, &runner);

    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");
}

#[test]
fn a_project_without_ci_has_green_at_the_approval() {
    let (rig, runner) = picking("all");
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);

    poll(&rig, &runner);

    assert_eq!(rig.forge.approvals(), [head]);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");
}

#[test]
fn a_new_push_resets_ci_to_waiting_and_is_not_approved_until_it_passes() {
    let (rig, runner) = blocking();
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude.script([
        Scripted::Text(CLEAN),
        Scripted::Fail(down),
        Scripted::Text(CLEAN),
    ]);
    let first = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    rig.forge.set_checks(&first, Checks::Passed);
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    rig.clock.advance(CHECKS_SETTLE);
    poll(&rig, &runner);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");

    // Pushed: the old head's verdict is not the new head's.
    let second = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    rig.forge.set_checks(&second, Checks::Passed);
    poll(&rig, &runner);
    let held = followed(&rig, &runner, 30);
    assert_eq!(held["head"], second);
    assert_eq!(held.get("ci"), None, "{held}");
    assert_eq!(rig.forge.approvals(), vec![first.clone()]);

    // The read that follows passes it, and the next poll approves it.
    poll(&rig, &runner);
    assert_eq!(followed(&rig, &runner, 30)["passed"], second);
    poll(&rig, &runner);

    assert_eq!(rig.forge.approvals(), [first, second]);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "waiting");
}

#[test]
fn a_run_held_after_the_listing_is_approved_while_the_settled_rollup_shows_nothing() {
    let (rig, runner) = blocking();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), vec![head.clone()]);

    // Inside the settling time the rollup is not read, so nothing is asked.
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), vec![head.clone()]);

    // Past it, no check reported: a run may have been held since the listing.
    rig.clock.advance(CHECKS_SETTLE);
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), [head.clone(), head.clone()]);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "waiting");

    // Once the rollup is read, the runs are no longer listed.
    rig.forge.set_checks(&head, Checks::Passed);
    poll(&rig, &runner);
    poll(&rig, &runner);
    assert_eq!(rig.forge.approvals(), [head.clone(), head]);
    assert_eq!(followed(&rig, &runner, 30)["ci"]["state"], "green");
}
