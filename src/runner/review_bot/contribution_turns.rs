//! A review bot's turn in the pass over a contribution's green head, through
//! the runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use super::REVIEW_WAIT;
use super::two_bots::{labels, summons_by_comment};
use crate::coderabbit::LABEL;
use crate::cubic::SUMMON;
use crate::lease::LeaseKind;
use crate::lease::wire::WindowFact;
use crate::ports::{CLEAN, Timestamp};
use crate::review_bot::Bot;
use crate::runner::coderabbit::tests::now;
use crate::runner::contributions::pass_tests::{asked, listing, read_through, write};
use crate::runner::contributions::security_tests::{commit_on, contribute, contribution};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted, Told};

const FINDING: &str = "P2: `bleats` loses a stamped prefix. Strip only files shep stamped.";
const NIT: &str = "P3: The name could be shorter.";

fn cubic() -> LeaseKind {
    Bot::Cubic.lease()
}

fn coderabbit() -> LeaseKind {
    Bot::Coderabbit.lease()
}

// Steps one poll on, and lets a call it began end
fn poll(rig: &Rig, runner: &Mutex<Runner>) {
    rig.next_look();
    step(runner).unwrap();
    crate::runner::contributions::budget_tests::settle(runner);
}

// Polls until cubic has been summoned on pull request 30
fn until_summoned(rig: &Rig, runner: &Mutex<Runner>) {
    for _ in 0..10 {
        if summons_by_comment(rig) > 0 {
            return;
        }
        poll(rig, runner);
    }
    panic!("cubic was never summoned: {}", contribution(rig, runner));
}

// Pull request 30, whose head has passed the security review and CI, with
// `list` as its reviewers, and its first reviewer's turn begun
fn contributed(list: &[&str]) -> (Rig, Mutex<Runner>, String) {
    let (rig, runner) = listing(list);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 2));
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    (rig, runner, head)
}

fn want_count(rig: &Rig, kind: &LeaseKind) -> usize {
    let told = rig.leases.told();
    told.iter()
        .filter(|t| **t == Told::Want(kind.clone()))
        .count()
}

fn ran(held: &Value) -> Vec<&str> {
    let all = held["pass"]["ran"].as_array().expect("a list");
    all.iter().map(|n| n.as_str().unwrap()).collect()
}

#[test]
fn a_bot_in_its_turn_is_summoned_once_on_the_head() {
    let (rig, runner, _head) = contributed(&["cubic", "mine"]);

    until_summoned(&rig, &runner);
    for _ in 0..12 {
        poll(&rig, &runner);
    }

    assert_eq!(summons_by_comment(&rig), 1, "the summon is not repeated");
    assert_eq!(want_count(&rig, &cubic()), 1);
    assert!(
        rig.leases
            .told()
            .iter()
            .any(|t| matches!(t, Told::Window(WindowFact::Summoned, _)))
    );
    assert_eq!(asked(&rig), Vec::<String>::new(), "the next waits its turn");
    assert_eq!(contribution(&rig, &runner).get("pass"), None);
}

#[test]
fn a_bot_review_on_the_head_ends_the_pass() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[]);

    let held = read_through(&rig, &runner, &head);

    assert_eq!(held["pass"]["answered"], "cubic");
    assert_eq!(ran(&held), ["cubic"]);
    assert_eq!(held["last_answered"], "cubic");
    assert_eq!(asked(&rig), Vec::<String>::new(), "no one reads after it");
    assert!(!rig.leases.held(&cubic()), "its lease went back");
}

#[test]
fn a_bot_out_of_its_window_is_skipped_and_the_next_reviewer_reads() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    rig.leases
        .opens_at(&cubic(), Timestamp(now(&rig) + 2 * 3600 + 600));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(summons_by_comment(&rig), 0, "it was never summoned");
    assert_eq!(want_count(&rig, &cubic()), 0);
    assert_eq!(asked(&rig), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
}

#[test]
fn a_bot_silent_for_two_hours_is_passed_over() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    poll(&rig, &runner);
    assert_eq!(asked(&rig), Vec::<String>::new(), "it is still waited on");

    rig.clock.advance(REVIEW_WAIT);
    let held = read_through(&rig, &runner, &head);

    assert_eq!(summons_by_comment(&rig), 1);
    assert_eq!(asked(&rig), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
    assert!(!rig.leases.held(&cubic()));
}

#[test]
fn the_local_backstop_reads_when_every_bot_is_skipped() {
    let (rig, runner, head) = contributed(&["cubic", "coderabbit", "mine"]);
    rig.leases
        .opens_at(&coderabbit(), Timestamp(now(&rig) + 3 * 3600));
    until_summoned(&rig, &runner);
    rig.forge.coderabbit.cubic_refuse(30, now(&rig) + 30);

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
    assert_eq!(held["pass"].get("unread"), None);
    assert_eq!(want_count(&rig, &coderabbit()), 0);
}

#[test]
fn a_bots_open_threads_above_a_nit_are_recorded_with_nits_apart() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[FINDING, NIT]);

    let held = read_through(&rig, &runner, &head);

    let findings = held["pass"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert!(findings[0]["what"].as_str().unwrap().contains("bleats"));
    assert_eq!(findings[0]["file"], "work.txt");
    let nits = held["pass"]["nits"].as_array().unwrap();
    assert_eq!(nits.len(), 1);
    assert_eq!(nits[0]["what"], "The name could be shorter.");
}

#[test]
fn no_comment_of_kelpies_is_posted_when_a_bots_findings_land() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[FINDING]);

    read_through(&rig, &runner, &head);

    assert_eq!(
        rig.forge.comments(),
        [(30, SUMMON.to_owned())],
        "the summon is the only comment"
    );
}

#[test]
fn a_new_push_starts_the_next_pass_after_the_bot_that_answered() {
    let (rig, runner, head) = contributed(&["cubic", "mine", "other"]);
    until_summoned(&rig, &runner);
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[FINDING]);
    read_through(&rig, &runner, &head);

    let pushed = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    let second = read_through(&rig, &runner, &pushed);

    assert_eq!(
        asked(&rig),
        ["mine"],
        "it began at the reviewer after cubic"
    );
    assert_eq!(second["pass"]["answered"], "mine");
    assert_eq!(summons_by_comment(&rig), 1, "cubic was not summoned again");
}

#[test]
fn a_bots_threads_on_one_head_are_not_carried_to_the_next() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[FINDING, NIT]);
    let first = read_through(&rig, &runner, &head);
    assert_eq!(first["pass"]["answered"], "cubic");

    let pushed = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    let second = read_through(&rig, &runner, &pushed);

    assert_eq!(second["pass"]["answered"], "mine");
    assert_eq!(second["pass"].get("findings"), None);
    assert_eq!(second["pass"].get("nits"), None);
}

#[test]
fn a_push_while_a_bot_is_summoned_takes_its_label_off_and_returns_its_lease() {
    let (rig, runner, _head) = contributed(&["coderabbit", "mine"]);
    for _ in 0..10 {
        if rig.leases.held(&coderabbit()) {
            break;
        }
        poll(&rig, &runner);
    }
    assert_eq!(labels(&rig).last(), Some(&(30, LABEL.to_owned(), true)));

    commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    poll(&rig, &runner);

    assert_eq!(labels(&rig).last(), Some(&(30, LABEL.to_owned(), false)));
    assert!(!rig.leases.held(&coderabbit()));
}

// Polls until `kind` is held for pull request 30's turn
fn until_held(rig: &Rig, runner: &Mutex<Runner>, kind: &LeaseKind) {
    for _ in 0..10 {
        if rig.leases.held(kind) {
            return;
        }
        poll(rig, runner);
    }
    panic!("{kind:?} was never taken: {}", contribution(rig, runner));
}

#[test]
fn a_contribution_passing_a_bot_over_leaves_the_lease_another_contributions_turn_holds() {
    let (rig, runner, _head) = contributed(&["cubic", "mine"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 2));
    contribute(&rig, 31, |h| write(h, "other.txt", "other\n"));
    until_summoned(&rig, &runner);
    rig.leases
        .opens_at(&cubic(), Timestamp(now(&rig) + 2 * 3600 + 600));

    for _ in 0..6 {
        poll(&rig, &runner);
    }

    assert_eq!(
        asked(&rig),
        ["mine"],
        "the other contribution's turn was passed over"
    );
    assert!(rig.leases.held(&cubic()), "the first turn still holds it");
    assert_eq!(want_count(&rig, &cubic()), 1);
}

#[test]
fn a_bot_that_cannot_be_read_for_two_hours_is_passed_over() {
    let (rig, runner, head) = contributed(&["coderabbit", "mine"]);
    until_held(&rig, &runner, &coderabbit());
    rig.forge.coderabbit.set_down(true);
    poll(&rig, &runner);
    assert_eq!(asked(&rig), Vec::<String>::new(), "it is still waited on");

    rig.clock.advance(REVIEW_WAIT);
    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
    assert!(!rig.leases.held(&coderabbit()));
    assert_eq!(labels(&rig).last(), Some(&(30, LABEL.to_owned(), false)));
}

#[test]
fn a_label_that_will_not_come_off_does_not_hold_the_pass() {
    let (rig, runner, head) = contributed(&["coderabbit", "mine"]);
    until_held(&rig, &runner, &coderabbit());
    rig.forge.set_labels_down(true);
    rig.forge.coderabbit.review(30, &head, now(&rig) + 30, &[]);

    let held = read_through(&rig, &runner, &head);

    assert_eq!(held["pass"]["answered"], "coderabbit");
    assert_eq!(asked(&rig), Vec::<String>::new());
}
