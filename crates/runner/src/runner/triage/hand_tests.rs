//! The maintainer's hand-labelled `ready-for-agent` issues taken back for
//! triage, through the runner's stand-ins

use std::sync::Mutex;

use super::ask_tests::{A_WORDS, asked, filed_before, record, settled_ready, wrote};
use super::pick_tests::{MAINTAINER, issue, list, rig_with};
use super::research_tests::{calls, moves, nothing, reply};
use crate::github::ApiError;
use crate::ports::Role;
use crate::runner::slots_tests::{ASKS, dispatched, issue_of};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

const HAND: &str = "pick = \"maintainer\"\nhand_ready = true";

// Issue 30 by `author`, as the forge lists it with `ready-for-agent` put on by hand
fn hand_labelled(rig: &Rig, author: &str) {
    let mut crash = issue(rig, 30, author, 5);
    crash.title = "Crash on start".to_owned();
    crash.body = "It crashes when I run it.".to_owned();
    crash.labels = vec!["ready-for-agent".to_owned()];
    list(rig, crash);
}

// Issue 30 as the forge lists it once `labels` are on it
fn relist(rig: &Rig, labels: &[&str]) {
    let mut listed = issue(rig, 30, MAINTAINER, 5);
    listed.title = "Crash on start".to_owned();
    listed.body = "It crashes when I run it.".to_owned();
    listed.labels = labels.iter().map(|&l| l.to_owned()).collect();
    list(rig, listed);
}

// A rig whose first triager reply asks a question, with issue 30 labelled by hand
// and ready on the board, which is what the board would start
fn on_the_board(lines: &str, author: &str) -> (Rig, Mutex<Runner>) {
    filed_before(lines, author, asked(), |rig| {
        hand_labelled(rig, author);
        rig.forge.list_ready(30, false);
    })
}

#[test]
fn a_hand_labelled_maintainer_issue_ends_the_poll_at_needs_triage_and_the_board_does_not_start_it()
{
    let (rig, runner) = on_the_board(HAND, MAINTAINER);

    assert_eq!(
        step(&runner).unwrap(),
        None,
        "the triage call, not the board"
    );

    assert_eq!(
        rig.github.label_takes(),
        [(30, "ready-for-agent".to_owned())]
    );
    assert_eq!(moves(&rig)[..2], ["label needs-triage", "comment"]);
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["working"],
        serde_json::json!([]),
        "no work item: {status}"
    );
}

#[test]
fn the_same_issue_is_started_by_the_board_while_hand_ready_is_off() {
    let (rig, runner) = on_the_board("pick = \"maintainer\"", MAINTAINER);

    let (started, _) = dispatched(step(&runner).unwrap());

    assert_eq!(started.github(), Some(30));
    assert_eq!(rig.github.label_takes(), []);
    assert_eq!(calls(&rig, Role::Triager).len(), 0);
}

#[test]
fn an_issue_someone_else_opened_is_left_for_the_board_even_with_hand_ready_on() {
    let (rig, runner) = on_the_board("pick = \"all\"\nhand_ready = true", "newcomer");

    let (started, _) = dispatched(step(&runner).unwrap());

    assert_eq!(started.github(), Some(30));
    assert_eq!(rig.github.label_takes(), []);
    assert_eq!(calls(&rig, Role::Triager).len(), 0);
}

#[test]
fn an_issue_with_a_work_item_is_never_taken_back() {
    let rig = rig_with(HAND);
    let runner = rig.open().unwrap();
    hand_labelled(&rig, MAINTAINER);
    rig.forge.list_ready(30, false);
    rig.claude.script([Scripted::Say(ASKS)]);
    rig.ask(&runner, "add", Some("30"));
    assert_eq!(issue_of(step(&runner).unwrap()).github(), Some(30));

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(rig.github.label_takes(), []);
    assert_eq!(calls(&rig, Role::Triager).len(), 0);
}

#[test]
fn an_issue_an_open_pull_request_closes_is_never_taken_back() {
    let (rig, runner) = on_the_board(HAND, MAINTAINER);
    rig.forge.list_open_pull(77, "Fix the crash", &[30]);

    step(&runner).unwrap();

    assert_eq!(rig.github.label_takes(), []);
    assert_eq!(calls(&rig, Role::Triager).len(), 0);
}

#[test]
fn its_answered_pass_makes_it_ready_again_and_the_next_poll_does_not_take_it_back() {
    // The forge as the swap leaves it, since the stand-in does not follow the App's writes
    let (rig, runner) = filed_before(HAND, MAINTAINER, asked(), |rig| {
        hand_labelled(rig, MAINTAINER);
        rig.forge.label(30, "needs-triage");
    });
    assert_eq!(step(&runner).unwrap(), None);
    relist(&rig, &["needs-triage"]);

    wrote(&rig, MAINTAINER, "Q1: Linux, on start. That is all of it.");
    rig.claude
        .script_for(Role::Triager, [reply(settled_ready())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let body = settled_ready()["body"]
        .as_str()
        .unwrap()
        .trim_end()
        .to_owned();
    assert_eq!(rig.github.issue_edits(), [(30, body)]);
    assert_eq!(moves(&rig).last().unwrap(), "label ready-for-agent");
    assert_eq!(record(&rig, &runner)["step"], "ready");
    let taken = rig.github.label_takes();

    relist(&rig, &["ready-for-agent"]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.github.label_takes(), taken, "not taken back again");
    assert_eq!(calls(&rig, Role::Triager).len(), 2);
}

#[test]
fn a_swap_the_forge_refuses_leaves_a_taken_record_and_is_finished_at_the_next_poll() {
    let (rig, runner) = filed_before(HAND, MAINTAINER, nothing(), |rig| {
        hand_labelled(rig, MAINTAINER)
    });
    rig.github.refuse_label_removals(ApiError::Refused(500));

    step(&runner).unwrap();

    let held = record(&rig, &runner);
    assert_eq!(held["step"], "taken", "{held}");
    assert_eq!(
        calls(&rig, Role::Triager).len(),
        0,
        "nothing is asked while it is ready"
    );

    rig.github.accept_label_removals();
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(moves(&rig)[..2], ["label needs-triage", "comment"]);
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    assert_eq!(record(&rig, &runner)["step"], "done");
}
