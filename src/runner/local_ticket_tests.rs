//! A project that takes its tickets from the local store, through the runner's
//! stand-ins, from `ticket new` to the merge

use std::sync::Mutex;

use serde_json::json;

use crate::board::Skip;
use crate::local_tickets::LocalTickets;
use crate::ports::{AgentError, CLEAN, Checks};
use crate::runner::slots_tests::dispatched;
use crate::runner::{CHECKS_SETTLE, Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted, git};
use crate::ticket::TicketId;

fn local(name: &str) -> TicketId {
    name.parse().unwrap()
}

// A running project on the local store, with a ready ticket `Add a thing` filed
fn filed(project: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.tickets_local();
    let filed = rig.ticket_verb(&[
        "new",
        "Add a thing",
        "--body",
        "Do it.",
        "--label",
        "ready-for-agent",
    ]);
    assert_eq!(filed, ["k1"]);
    let runner = rig.open().unwrap();
    (rig, runner)
}

fn state_of(rig: &Rig, ticket: &str) -> String {
    let shown = rig.ticket_verb(&["show", ticket]);
    let state = shown.iter().find_map(|line| line.strip_prefix("state: "));
    state.expect("the ticket has a state").to_owned()
}

#[test]
fn a_local_ticket_goes_from_ticket_new_to_a_merge_that_closes_it() {
    let (rig, runner) = filed("koji");
    rig.merge_auto();
    drop(runner);
    let runner = rig.open().unwrap();
    rig.forge.open_pull_request(71, "kelpie/k1", &[]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);

    let (issue, skipped) = dispatched(step(&runner).unwrap());
    assert_eq!((issue, skipped), (local("k1"), vec![]));

    step(&runner).unwrap(); // the worker's first turn
    let worktree = rig.paths().worktree(local("k1"));
    assert_eq!(git(&worktree, &["branch", "--show-current"]), "kelpie/k1");
    let [call] = rig.claude.calls().try_into().unwrap();
    assert!(
        call.prompt
            .ends_with("\nYour work item is ticket k1: Add a thing\n\nDo it.\n"),
        "{}",
        call.prompt
    );
    let instructions = rig.paths().worker.join("instructions-k1.md");
    let instructions = std::fs::read_to_string(instructions).unwrap();
    assert!(!instructions.contains("Resolves"), "{instructions}");
    assert_eq!(state_of(&rig, "k1"), "open");

    let mut head = None;
    for _ in 0..20 {
        step(&runner).unwrap();
        if rig.ask(&runner, "status", None)["work_item"]["phase"]["state"] == "ci" {
            head = rig.forge.head_of("kelpie/k1");
            break;
        }
    }
    let head = head.expect("the review never reached CI");
    rig.forge.set_checks(&head, Checks::Passed);
    assert!(
        rig.verdict(&runner).is_some(),
        "the pull request was marked ready"
    );
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap();

    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(state_of(&rig, "k1"), "closed");
    assert!(
        rig.forge.comments().is_empty(),
        "nothing was said on GitHub"
    );
    assert_eq!(rig.forge.issue_writes(), Vec::<String>::new());
    assert_eq!(
        local_comments(&rig, "k1"),
        ["Pull request #71 merged, so kelpie closes this ticket."]
    );
}

fn local_comments(rig: &Rig, ticket: &str) -> Vec<String> {
    let store = LocalTickets::new(rig.paths().tickets());
    let stored = store.get(local(ticket)).unwrap();
    stored.comments.into_iter().map(|c| c.body).collect()
}

#[test]
fn a_ruling_before_there_is_a_pull_request_goes_on_the_local_ticket() {
    let (rig, runner) = filed("koji");
    dispatched(step(&runner).unwrap());
    rig.claude.script([Scripted::Fail(AgentError::Failed(
        Harness::ClaudeCode,
        "overloaded".into(),
    ))]);
    let Some(StepReport::Failed { .. }) = step(&runner).unwrap() else {
        panic!("the worker's call did not fail");
    };

    let [said] = local_comments(&rig, "k1").try_into().unwrap();
    assert!(said.ends_with("Waiting on the maintainer."), "{said}");
    assert!(rig.forge.comments().is_empty());
    assert_eq!(rig.forge.issue_writes(), Vec::<String>::new());
}

#[test]
fn adopt_is_refused_on_a_local_store_saying_it_needs_github_issues() {
    let (rig, runner) = filed("koji");
    rig.forge.open_pull_request(71, "feature", &[]);
    let refused = rig.ask(&runner, "adopt", Some("71"))["error"].to_string();
    assert!(
        refused.contains("adoption needs GitHub issues"),
        "{refused}"
    );
    assert!(rig.forge.comments().is_empty());
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn a_ticket_held_on_a_local_store_gains_no_label_on_the_repo() {
    let (rig, runner) = filed("koji");
    dispatched(step(&runner).unwrap());
    assert_eq!(rig.forge.issue_writes(), Vec::<String>::new());
    drop(runner);
    let stored = LocalTickets::new(rig.paths().tickets()).get(local("k1"));
    assert!(!stored.unwrap().labels.iter().any(|l| l == "in-progress"));
}

// Tickets `k1`, which `k2` blocks, and `k2`, both ready
fn blocked_by_the_next() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("koji");
    rig.tickets_local();
    for title in ["Needs the second", "The second"] {
        rig.ticket_verb(&["new", title, "--label", "ready-for-agent"]);
    }
    rig.ticket_verb(&["edit", "k1", "--blocked-by", "k2"]);
    let runner = rig.open().unwrap();
    (rig, runner)
}

#[test]
fn a_blocked_local_ticket_is_skipped_while_its_blocker_is_open() {
    let (_rig, runner) = blocked_by_the_next();
    let (issue, skipped) = dispatched(step(&runner).unwrap());
    assert_eq!(issue, local("k2"));
    assert_eq!(
        skipped,
        [Skip::Blocked {
            issue: local("k1"),
            by: vec![local("k2")],
            unlisted: 0,
        }]
    );
}

#[test]
fn a_blocked_local_ticket_is_taken_once_its_blocker_closes() {
    let (rig, runner) = blocked_by_the_next();
    rig.ticket_verb(&["close", "k2"]);
    let (issue, skipped) = dispatched(step(&runner).unwrap());
    assert_eq!((issue, skipped), (local("k1"), vec![]));
}

#[test]
fn a_ticket_the_store_does_not_hold_opens_no_work_item() {
    let (rig, runner) = filed("koji");
    let error = rig.ask(&runner, "add", Some("k12"))["error"].to_string();
    assert!(error.contains("cannot read the issue"), "{error}");
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn a_github_ticket_still_opens_a_work_item_on_a_local_project() {
    let (rig, runner) = filed("koji");
    let added = rig.ask(&runner, "add", Some("12"));
    assert_eq!(added["error"], json!(null), "{added}");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["issue"],
        json!("12")
    );
}
