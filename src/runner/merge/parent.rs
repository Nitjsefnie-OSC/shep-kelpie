//! A merge closing the parent of its issue, through the runner's stand-ins

use std::sync::Mutex;

use crate::ports::PullRequestState;
use crate::runner::gate::CHECKS_SETTLE;
use crate::runner::{Runner, StepReport, step};
use crate::test::Rig;

// Rules a yes on the parked pull request for issue 7, then merges it
fn merged(rig: &Rig, runner: &Mutex<Runner>) {
    rig.ask(runner, "rule", Some("1 yes"));
    assert!(matches!(
        step(runner).unwrap(),
        Some(StepReport::MarkedReady { .. })
    ));
    rig.clock.advance(CHECKS_SETTLE);
    assert!(matches!(
        step(runner).unwrap(),
        Some(StepReport::Finished { merged: true, .. })
    ));
}

#[test]
fn merging_the_last_open_sub_issue_closes_its_parent_naming_the_pull_request() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.list_ready(5, false);
    rig.forge.link_sub_issue(5, 6);
    rig.forge.link_sub_issue(5, 7);
    rig.forge.close_issue(6);

    merged(&rig, &runner);
    assert_eq!(rig.forge.closings(), [5]);
    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert_eq!(comments[0].0, 5);
    assert!(comments[0].1.contains("#71"), "{comments:?}");
}

#[test]
fn a_merge_that_leaves_another_sub_issue_open_closes_nothing() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.link_sub_issue(5, 6);
    rig.forge.link_sub_issue(5, 7);

    merged(&rig, &runner);
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}

#[test]
fn a_parent_whose_sub_issues_were_all_closed_as_not_planned_stays_open() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.link_sub_issue(5, 6);
    rig.forge.link_sub_issue(5, 7);
    rig.forge.close_issue_as_not_planned(6);
    rig.forge.close_issue_as_not_planned(7);

    merged(&rig, &runner);
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}

#[test]
fn a_parent_with_a_built_sub_issue_closes_though_another_was_not_planned() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.list_ready(5, false);
    rig.forge.link_sub_issue(5, 6);
    rig.forge.link_sub_issue(5, 7);
    rig.forge.close_issue_as_not_planned(6);

    merged(&rig, &runner);
    assert_eq!(rig.forge.closings(), [5]);
}

#[test]
fn a_forge_that_fails_the_parent_read_still_finishes_the_merge_and_logs_it() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.link_sub_issue(5, 7);
    rig.forge.set_sub_issues_down(true);

    merged(&rig, &runner);
    assert!(rig.forge.closings().is_empty());
    let notes = runner.lock().unwrap().take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("cannot read the sub-issues of #5"),
        "{notes:?}"
    );
}

#[test]
fn a_closed_parent_does_not_close_its_own_parent() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.list_ready(5, false);
    rig.forge.link_sub_issue(3, 5);
    rig.forge.link_sub_issue(5, 7);

    merged(&rig, &runner);
    assert_eq!(rig.forge.closings(), [5]);
}

#[test]
fn a_parent_the_forge_already_closed_gets_no_second_close() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.link_sub_issue(5, 7);
    rig.forge.close_issue(5);

    merged(&rig, &runner);
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}

#[test]
fn a_sibling_the_pull_request_names_counts_as_closed_though_it_still_reads_open() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.list_ready(5, false);
    rig.forge.link_sub_issue(5, 6);
    rig.forge.link_sub_issue(5, 7);
    rig.forge.set_closes(71, &[6, 7]);
    rig.ask(&runner, "rule", Some("1 yes"));
    // The forge has merged it and not yet closed #6 and #7, which #71 names.
    rig.forge.set_state(71, PullRequestState::Merged);

    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Finished { merged: true, .. })
    ));
    assert_eq!(rig.forge.closings(), [5]);
}
