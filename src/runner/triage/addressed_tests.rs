//! An issue another open issue or pull request already covers, which kelpie
//! closes as a duplicate naming it, through the runner's stand-ins

use serde_json::json;

use super::comment::MARKER;
use super::pick_tests::{issue, list};
use super::research_tests::{
    WORDS, calls, crashed_with, facts, moves, pump, reply, triage, untouched,
};
use crate::ports::{IssueCloseReason, PullRequestState, Role};
use crate::runner::{Pass, advance, step};
use crate::test::{CALL_PATIENCE, Hold, Scripted};

// The triager's reply naming `#number`, of `kind`, as covering the issue
fn covered(number: u64, kind: &str, why: &str) -> Scripted {
    reply(json!({
        "addressed_by": { "number": number, "kind": kind, "why": why },
    }))
}

#[test]
fn an_issue_an_open_pull_request_resolves_is_closed_as_duplicate_with_a_comment_naming_it() {
    let (rig, runner) = untouched();
    rig.forge
        .list_open_pull(41, "Fix the crash on start", &[30]);
    rig.claude.script_for(
        Role::Triager,
        [covered(41, "pull_request", "It fixes the same crash.")],
    );

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))],
        "one short comment, worded from what the research named"
    );
    let said = facts(&rig);
    assert!(said.contains("#41"), "{said}");
    assert!(said.contains("pull request"), "{said}");
    assert!(said.contains("It fixes the same crash."), "{said}");
    assert_eq!(rig.forge.closings(), [30]);
    assert_eq!(
        rig.forge.closing_reason(30),
        Some(IssueCloseReason::Duplicate),
        "closed as a duplicate, not as completed"
    );
    let triaged = triage(&rig, &runner);
    assert_eq!(triaged["issues"][0]["issue"], 30);
    assert_eq!(triaged["issues"][0]["step"], "done");
    assert_eq!(triaged["issues"][0]["duplicate_of"], 41);
}

#[test]
fn an_issue_another_open_issue_covers_is_closed_as_duplicate() {
    let (rig, runner) = untouched();
    let mut other = issue(&rig, 12, "someone", 0);
    other.title = "Add retries".to_owned();
    list(&rig, other);
    rig.claude.script_for(
        Role::Triager,
        [covered(12, "issue", "It asks for the same retry.")],
    );

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.forge.closings(), [30]);
    assert_eq!(
        rig.forge.closing_reason(30),
        Some(IssueCloseReason::Duplicate)
    );
    let said = facts(&rig);
    assert!(said.contains("#12"), "{said}");
    assert!(said.contains("It asks for the same retry."), "{said}");
    assert_eq!(triage(&rig, &runner)["issues"][0]["duplicate_of"], 12);
}

#[test]
fn a_number_not_in_the_listing_or_closed_or_itself_is_dropped_and_the_issue_is_triaged() {
    // `named` is what the reply gives, `other` what the forge holds beside it
    let cases = [
        ("no item of that number is open at all", 99, "issue", None),
        (
            "the pull request it named is closed",
            41,
            "pull_request",
            Some(true),
        ),
        ("it named the issue itself", 30, "issue", None),
    ];
    for (what, number, kind, closed) in cases {
        let (rig, runner) = untouched();
        rig.forge
            .list_open_pull(41, "Fix the crash on start", &[30]);
        if closed == Some(true) {
            rig.forge.set_state(41, PullRequestState::Closed);
        }
        rig.claude
            .script_for(Role::Triager, [covered(number, kind, "It covers it.")]);

        assert_eq!(step(&runner).unwrap(), None, "{what}");

        assert_eq!(rig.forge.closings(), Vec::<u64>::new(), "{what}");
        assert_eq!(
            rig.github.comments(),
            [(30, format!("{WORDS}\n\n{MARKER}"))],
            "{what}"
        );
        assert_eq!(
            moves(&rig),
            ["comment", "make needs-triage", "label needs-triage"],
            "{what}"
        );
        let triaged = triage(&rig, &runner);
        assert_eq!(triaged["issues"][0]["step"], "done", "{what}");
        assert!(triaged["issues"][0].get("duplicate_of").is_none(), "{what}");
        let told: Vec<String> = runner.lock().unwrap().take_notes();
        assert!(
            told.iter().any(|n| n.starts_with("triage #30:")
                && n.contains(&format!("names #{number}"))
                && n.contains("triaged as any other")),
            "{what}: {told:?}"
        );
    }
}

#[test]
fn a_number_the_call_was_never_shown_is_dropped_even_when_it_is_open() {
    let (rig, runner) = untouched();
    let hold = Hold::default();
    let said = json!({
        "addressed_by": { "number": 55, "kind": "issue", "why": "It names the same crash." },
    })
    .to_string();
    rig.claude.script_for(
        Role::Triager,
        [Scripted::HoldThenSay(
            hold.clone(),
            Box::leak(said.into_boxed_str()),
        )],
    );

    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the research never began");

    // Issue 55 opens while the call runs: it is open at the post, and it was
    // never in the listing the call was given.
    let mut late = issue(&rig, 55, "someone", 0);
    late.title = "Also crashes".to_owned();
    list(&rig, late);
    hold.release();
    pump(&runner, || {
        triage(&rig, &runner)["issues"][0]["step"] == "done"
    });

    assert_eq!(rig.forge.closings(), Vec::<u64>::new());
    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))],
        "an ordinary triage comment"
    );
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        told.iter().any(|n| n.starts_with("triage #30:")
            && n.contains("names #55")
            && n.contains("was in its listing")),
        "{told:?}"
    );
}

#[test]
fn a_closed_duplicate_gets_no_labels() {
    let (rig, runner) = untouched();
    rig.forge.set_repo_labels(&["bug", "P2", "needs-triage"]);
    rig.forge
        .list_open_pull(41, "Fix the crash on start", &[30]);
    rig.claude.script_for(
        Role::Triager,
        [reply(json!({
            "type": "bug",
            "priority": "P2",
            "addressed_by": {
                "number": 41, "kind": "pull_request", "why": "It fixes the same crash."
            },
        }))],
    );

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(moves(&rig), ["comment"], "no label of any kind");
    assert_eq!(rig.forge.closings(), [30]);
    assert_eq!(
        rig.forge.closing_reason(30),
        Some(IssueCloseReason::Duplicate)
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
}

#[test]
fn a_restart_after_the_comment_closes_it_without_posting_again() {
    let (rig, runner) = untouched();
    drop(runner);
    crashed_with(
        &rig,
        json!({ "issue": 30, "comment": 101, "step": "closed", "duplicate_of": 41 }),
    );
    let runner = rig.open().unwrap();
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.claude.all_seen().len(), 0, "no call and no new wording");
    assert_eq!(rig.github.comments(), []);
    assert_eq!(
        moves(&rig),
        Vec::<String>::new(),
        "a closed duplicate carries no label"
    );
    assert_eq!(rig.forge.closings(), [30]);
    assert_eq!(
        rig.forge.closing_reason(30),
        Some(IssueCloseReason::Duplicate),
        "the close the record says is owed"
    );
    let triaged = triage(&rig, &runner);
    assert_eq!(triaged["issues"][0]["step"], "done");
    assert_eq!(triaged["issues"][0]["duplicate_of"], 41);
}

#[test]
fn a_closed_duplicate_is_not_triaged_again_while_its_record_is_kept() {
    let (rig, runner) = untouched();
    rig.forge
        .list_open_pull(41, "Fix the crash on start", &[30]);
    rig.claude.script_for(
        Role::Triager,
        [covered(41, "pull_request", "It fixes the same crash.")],
    );

    assert_eq!(step(&runner).unwrap(), None);

    // The author reopens it: it carries no triage label, and kelpie has its
    // record, so no pass is started on kelpie's own.
    for _ in 0..2 {
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
    }
    assert_eq!(calls(&rig, Role::Triager).len(), 1, "researched once");
    assert_eq!(rig.github.comments().len(), 1);
    assert_eq!(rig.forge.closings(), [30], "closed once");
}

#[test]
fn the_pull_requests_and_issues_reach_the_call_fenced_and_without_the_issue_itself() {
    let (rig, runner) = untouched();
    for (number, title) in [(12, "Add retries"), (13, "Crash\non exit")] {
        let mut other = issue(&rig, number, "someone", 0);
        other.title = title.to_owned();
        list(&rig, other);
    }
    rig.forge
        .list_open_pull(41, "Fix the crash on start", &[30]);
    rig.forge.list_open_pull(42, "Rework the docs", &[]);
    rig.claude.script_for(Role::Triager, [reply(json!({}))]);

    assert_eq!(step(&runner).unwrap(), None);

    let asked = calls(&rig, Role::Triager);
    let prompt = &asked[0].call.prompt;
    let fenced_issues = "UNTRUSTED-0 begin\n#12 Add retries\n#13 Crash on exit\nUNTRUSTED-0 end";
    assert!(prompt.contains(fenced_issues), "{prompt}");
    let fenced_pulls = "UNTRUSTED-0 begin\n#41 Fix the crash on start (closes #30)\n\
                        #42 Rework the docs\nUNTRUSTED-0 end";
    assert!(prompt.contains(fenced_pulls), "{prompt}");
    assert!(
        !prompt.contains("#30 Crash on start"),
        "the issue is not listed against itself: {prompt}"
    );
}
