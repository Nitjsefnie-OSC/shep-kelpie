//! The pass's one listing of open pull requests, through the runner's stand-ins

use std::sync::atomic::Ordering;

use crate::board::READY;
use crate::ports::{Checks, MaintainerReview};
use crate::runner::{CHECKS_SETTLE, StepReport, step};
use crate::test::Rig;

// What the forge was asked of pull requests so far: listings, single reads
// and reviews
fn reads(rig: &Rig) -> (usize, usize, usize) {
    let reads = rig.forge.pull_request_reads();
    (
        reads.listed.load(Ordering::SeqCst),
        reads.viewed.load(Ordering::SeqCst),
        reads.reviewed.load(Ordering::SeqCst),
    )
}

fn a_review(id: &str) -> MaintainerReview {
    MaintainerReview {
        id: id.into(),
        changes_requested: false,
        body: "Looks fine.".into(),
        comments: vec![],
    }
}

fn six_open(rig: &Rig) {
    for number in 61..=66 {
        let issue = number - 50;
        rig.forge
            .open_pull_request(number, &format!("kelpie/{issue}"), &[issue]);
    }
}

#[test]
fn a_board_read_of_six_open_pull_requests_is_one_query_and_nothing_after_it() {
    let rig = Rig::new("pikachu");
    six_open(&rig);
    let runner = rig.open().unwrap();
    assert_eq!(reads(&rig), (0, 0, 0));

    step(&runner).unwrap();

    assert_eq!(
        reads(&rig),
        (1, 0, 0),
        "one listing, and no pull request read again for the rework check"
    );
}

#[test]
fn a_review_is_read_in_full_once_for_each_time_one_arrives() {
    let rig = Rig::new("eevee");
    six_open(&rig);
    rig.forge.review(61, a_review("PRR_61"));
    let runner = rig.open().unwrap();

    step(&runner).unwrap();
    assert_eq!(reads(&rig), (1, 0, 1), "the one with a review is read");

    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(reads(&rig), (2, 0, 1), "the same review is not read again");

    rig.forge.review(61, a_review("PRR_61_again"));
    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(reads(&rig), (3, 0, 2), "a new one is");
}

#[test]
fn a_pull_request_with_the_ready_label_is_read_in_full() {
    let rig = Rig::new("snorlax");
    six_open(&rig);
    rig.forge.label_pull_request(61, READY);
    let runner = rig.open().unwrap();

    step(&runner).unwrap();

    assert_eq!(reads(&rig), (1, 0, 1), "the labelled one, and no other");
}

#[test]
fn the_ci_gate_reads_the_pass_listing_not_the_pull_request() {
    let (rig, runner, head) = Rig::with_pull_request("bulbasaur");
    let (before_listed, before_viewed, _) = reads(&rig);

    // CI has not finished, so the gate looks and waits.
    assert_eq!(step(&runner).unwrap(), None);
    let (listed, viewed, _) = reads(&rig);
    assert_eq!(viewed, before_viewed, "no single pull request read");
    assert_eq!(listed, before_listed + 1, "the gate read the listing");

    // The verdict hands the pull request back, which reads the labels it is
    // about to change from the forge as they stand.
    rig.forge.set_checks(&head, Checks::Passed);
    let report = rig.verdict(&runner);
    assert!(
        matches!(report, Some(StepReport::Ruling { id: 71, .. })),
        "{report:?}"
    );
    let (_, viewed, _) = reads(&rig);
    assert_eq!(viewed, before_viewed + 1, "the hand-back's read alone");
}

#[test]
fn a_merge_reads_the_listing_and_still_merges_the_head_it_was_asked_about() {
    let (rig, runner, head) = Rig::with_pull_request("charmander");
    rig.merge_auto();
    drop(runner);
    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    rig.verdict(&runner);
    rig.clock.advance(CHECKS_SETTLE);
    let (_, before_viewed, _) = reads(&rig);

    let report = step(&runner).unwrap();

    assert!(
        matches!(report, Some(StepReport::Finished { merged: true, .. })),
        "{report:?}"
    );
    assert_eq!(rig.forge.merges(), [(71, head)], "the merge names its head");
    let (_, viewed, _) = reads(&rig);
    assert_eq!(
        viewed - before_viewed,
        1,
        "only the read after the merge, which tells whether it landed"
    );
}

#[test]
fn a_head_that_moved_since_the_gate_is_seen_in_the_listing_and_not_merged() {
    let (rig, runner, head) = Rig::with_pull_request("squirtle");
    rig.merge_auto();
    drop(runner);
    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    rig.verdict(&runner);
    rig.clock.advance(CHECKS_SETTLE);
    rig.push_by_hand("kelpie/7", "late.txt");

    let report = step(&runner).unwrap();

    assert!(
        matches!(report, Some(StepReport::MergeWithdrawn { .. })),
        "{report:?}"
    );
    assert_eq!(rig.forge.merges(), []);
}
