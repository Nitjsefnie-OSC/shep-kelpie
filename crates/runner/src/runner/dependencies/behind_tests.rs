//! A bump behind `main` is asked to rebase only when it conflicts or the repo's rules require it

use super::super::contributions::security_tests::commit_on;
use super::tests::{bump, bumping, bumps, notes, poll, settle, words, write};
use crate::ports::Checks;

#[test]
fn a_behind_bump_is_merged_without_a_rebase_ask_when_main_does_not_require_up_to_date() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&head, Checks::Passed);
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_required(false);

    settle(&rig, &runner);

    assert!(rig.forge.comments().is_empty(), "no rebase is asked");
    assert_eq!(rig.forge.merges(), [(40, head)]);
    let notes = notes(&runner);
    assert_eq!(words(&notes, "bump #40: merged"), 1, "{notes:?}");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("behind main, which does not require it to be up to date")),
        "{notes:?}"
    );
}

#[test]
fn a_behind_bump_with_red_checks_is_closed_when_main_does_not_require_up_to_date() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_required(false);

    settle(&rig, &runner);

    let comments = rig.forge.comments();
    assert_eq!(comments.len(), 1, "the red comment, not a rebase ask");
    assert!(
        comments[0].1.contains("check `test` failed"),
        "{comments:?}"
    );
    assert_eq!(rig.forge.pull_request_closings(), [40]);
    assert_eq!(rig.forge.created().len(), 1);
}

#[test]
fn a_behind_bump_names_the_rule_that_let_it_through_in_its_standing() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&head, Checks::Pending(Vec::new()));
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_required(false);

    settle(&rig, &runner);

    let listed = bumps(&rig, &runner);
    let standing = listed[0]["standing"].as_str().unwrap();
    assert!(
        standing.contains("behind main, which does not require it to be up to date"),
        "{standing}"
    );
    assert!(
        standing.contains("waiting for its checks to finish"),
        "{standing}"
    );
}

#[test]
fn a_conflicting_bump_is_asked_to_rebase_once_for_each_head_when_main_does_not_require_up_to_date()
{
    let (rig, runner) = bumping("");
    let stale = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&stale, Checks::Passed);
    rig.land("Cargo.lock", "main changed it\n");
    rig.forge.set_up_to_date_required(false);

    settle(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(
        rig.forge.comments(),
        [(40, "@dependabot rebase".to_owned())]
    );
    assert!(rig.forge.merges().is_empty());
    let listed = bumps(&rig, &runner);
    let standing = listed[0]["standing"].as_str().unwrap();
    assert!(standing.contains("conflicts"), "{standing}");

    let fresh = commit_on(&rig, 40, &format!("{stale}^"), |h| {
        write(h, "Cargo.lock", "bumped again\n");
    });
    rig.forge.set_checks(&fresh, Checks::Passed);
    poll(&rig, &runner);

    assert_ne!(fresh, stale);
    assert_eq!(rig.forge.comments().len(), 2, "the new head is asked once");
    assert!(rig.forge.merges().is_empty(), "it still conflicts");
}

#[test]
fn a_behind_bump_is_asked_to_rebase_when_the_rule_cannot_be_read() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&head, Checks::Passed);
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_read_down(true);

    settle(&rig, &runner);

    assert_eq!(
        rig.forge.comments(),
        [(40, "@dependabot rebase".to_owned())]
    );
    let listed = bumps(&rig, &runner);
    let standing = listed[0]["standing"].as_str().unwrap();
    assert!(standing.contains("cannot tell its rule"), "{standing}");
}

#[test]
fn a_web_bump_behind_main_is_proved_when_main_does_not_require_up_to_date() {
    let (rig, runner) = bumping("web = [\"web/\"]");
    let head = bump(&rig, 50, "web/package-lock.json");
    rig.forge.set_checks(&head, Checks::Passed);
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_required(false);

    settle(&rig, &runner);

    assert_eq!(
        rig.e2e.ran().len(),
        1,
        "the web proof runs on the behind head"
    );
    assert!(rig.forge.merges().is_empty(), "the run has just ended");

    poll(&rig, &runner);

    assert_eq!(rig.forge.merges(), [(50, head)]);
}

#[test]
fn the_up_to_date_rule_is_read_only_when_a_bump_is_behind() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&head, Checks::Pending(Vec::new()));

    settle(&rig, &runner);
    assert_eq!(rig.forge.up_to_date_reads(), 0, "the bump is on main");

    rig.land("main.txt", "main moved on\n");
    poll(&rig, &runner);
    let asked = rig.forge.up_to_date_reads();
    assert_eq!(asked, 1, "one read for the poll");
    poll(&rig, &runner);
    assert_eq!(rig.forge.up_to_date_reads(), asked + 1, "one read per poll");
}

#[test]
fn a_behind_bump_the_forge_refuses_to_merge_is_asked_to_rebase_once_for_its_head() {
    let (rig, runner) = bumping("");
    let head = bump(&rig, 40, "Cargo.lock");
    rig.forge.set_checks(&head, Checks::Passed);
    rig.land("main.txt", "main moved on\n");
    rig.forge.set_up_to_date_required(false);
    rig.forge.set_merges_down(true);

    settle(&rig, &runner);

    assert!(rig.forge.merges().is_empty());
    assert_eq!(
        rig.forge.comments(),
        [(40, "@dependabot rebase".to_owned())],
        "the refusal reads as a rule kelpie cannot tell: ask"
    );
    let notes = notes(&runner);
    assert_eq!(
        words(&notes, "bump #40: asked dependabot to rebase"),
        1,
        "{notes:?}"
    );
    let listed = bumps(&rig, &runner);
    let standing = listed[0]["standing"].as_str().unwrap();
    assert!(standing.contains("cannot tell its rule"), "{standing}");
    assert!(
        standing.contains("the forge refused the merge"),
        "{standing}"
    );
    assert!(standing.contains("asked to rebase"), "{standing}");

    poll(&rig, &runner);

    assert_eq!(rig.forge.comments().len(), 1, "asked once for this head");
    let listed = bumps(&rig, &runner);
    let standing = listed[0]["standing"].as_str().unwrap();
    assert!(
        standing.contains("asked to rebase, and has not pushed yet"),
        "{standing}"
    );
}
