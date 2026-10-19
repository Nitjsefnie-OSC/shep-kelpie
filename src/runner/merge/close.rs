//! Closing the issue of a merged pull request through the runner's stand-ins

use serde_json::json;

use super::tests::{finished, ready_then_merge};
use crate::ports::PullRequestState;
use crate::runner::{StepReport, step};
use crate::test::Rig;
use crate::test::ticket;

#[test]
fn a_pull_request_merged_without_a_closing_reference_gets_its_issue_closed() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.set_closes(71, &[]);
    rig.forge.list_ready(7, false);
    rig.forge.list_ready(8, false);
    rig.forge.block(8, 7);
    rig.ask(&runner, "rule", Some("71 yes"));

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert_eq!(rig.forge.closings(), [7]);
    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert_eq!(comments[0].0, 7);
    assert!(comments[0].1.contains("#71"), "{comments:?}");
    rig.next_look();
    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::Dispatched { issue, .. }) if issue == ticket(8)
    ));
}

#[test]
fn a_pull_request_the_maintainer_merged_by_hand_gets_its_issue_closed_too() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.set_closes(71, &[]);
    rig.forge.list_ready(7, false);
    rig.ask(&runner, "rule", Some("71 yes"));
    rig.forge.set_state(71, PullRequestState::Merged);

    assert!(finished(step(&runner).unwrap()));
    assert_eq!(rig.forge.closings(), [7]);
    assert_eq!(rig.forge.comments().len(), 1);
}

#[test]
fn an_issue_the_pull_request_names_is_left_to_the_forge_though_it_still_reads_open() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.list_ready(7, false);
    rig.ask(&runner, "rule", Some("71 yes"));
    // The forge has merged it and not yet closed #7, which #71 names.
    rig.forge.set_state(71, PullRequestState::Merged);

    assert!(finished(step(&runner).unwrap()));
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}

#[test]
fn an_issue_the_forge_already_closed_gets_no_comment_and_no_second_close() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}

#[test]
fn a_close_the_forge_refuses_is_tried_once_in_each_step_and_then_given_up() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.set_closes(71, &[]);
    rig.forge.list_ready(7, false);
    rig.forge.set_closes_down(true);
    rig.ask(&runner, "rule", Some("71 yes"));

    let first = ready_then_merge(&rig, &runner);
    assert!(
        matches!(&first, Some(StepReport::GateFailed { issue, reason })
            if *issue == ticket(7)
                && reason.contains("cannot close issue #7")
                && reason.contains("closing is refused")),
        "{first:?}"
    );
    assert_eq!(rig.forge.closes_refused(), 1);
    assert_ne!(rig.ask(&runner, "status", None)["work_item"], json!(null));

    // The step runs again after its backoff, and tries the close once.
    rig.clock.advance(15);
    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::GateFailed { issue, .. }) if issue == ticket(7)
    ));
    assert_eq!(rig.forge.closes_refused(), 2);

    // The third refusal gives the issue up, and the work item finishes.
    rig.clock.advance(30);
    assert!(finished(step(&runner).unwrap()));
    assert_eq!(rig.forge.closes_refused(), 3);
    assert!(rig.forge.closings().is_empty());
    assert_eq!(rig.forge.comments().len(), 1, "the comment goes once");
    let notes = runner.lock().unwrap().take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("cannot close issue #7"), "{notes:?}");
    assert!(notes[0].contains("closing is refused"), "{notes:?}");
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn a_comment_the_forge_refuses_is_noted_and_the_issue_is_closed_without_it() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.set_closes(71, &[]);
    rig.forge.list_ready(7, false);
    rig.forge.set_comments_down(true);
    rig.ask(&runner, "rule", Some("71 yes"));

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert_eq!(rig.forge.closings(), [7]);
    let notes = runner.lock().unwrap().take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("cannot comment on issue #7"), "{notes:?}");
}

#[test]
fn an_item_that_ended_without_a_merge_leaves_its_issue_untouched() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.forge.set_closes(71, &[]);
    rig.forge.list_ready(7, false);

    rig.ask(&runner, "drop", Some("7"));
    assert!(rig.forge.closings().is_empty());
    assert!(rig.forge.comments().is_empty());
}
