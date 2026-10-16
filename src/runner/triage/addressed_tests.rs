//! An issue another open issue or pull request already covers, which kelpie
//! closes as a duplicate naming it, through the runner's stand-ins

use serde_json::json;

use super::comment::MARKER;
use super::pick_tests::{issue, list};
use super::research_tests::{WORDS, calls, facts, reply, triage, untouched};
use crate::ports::{IssueCloseReason, Role};
use crate::runner::step;
use crate::test::Scripted;

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
