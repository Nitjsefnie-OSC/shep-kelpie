//! The maintainer's own issue the pass is sure of, made agent-ready through
//! the runner's stand-ins

use std::sync::Mutex;

use serde_json::{Value, json};

use super::comment::MARKER;
use super::pick_tests::{MAINTAINER, issue, list, rig_with};
use super::research_tests::{
    WORDS, calls, crashed_with, facts, moves, nothing, pump, reply, triage,
};
use crate::github::ApiError;
use crate::ports::Role;
use crate::runner::{Pass, Runner, advance, step};
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

// What the triager is sure of: a type, a priority, no open question, and an
// agent-ready body
const BODY: &str = "Touches:\n- `src/start.rs` (new)\n\n## What to build\n\n- Fix the \
    crash on start.\n\n## Acceptance criteria\n\n- [ ] `runner::start::it_starts`\n";

fn sure() -> Value {
    json!({ "type": "bug", "priority": "P2", "body": BODY })
}

// The same reply as leaked text, for a scripted call held open
fn said(found: Value) -> &'static str {
    let mut all = nothing();
    for (key, value) in found.as_object().expect("an object") {
        all[key] = value.clone();
    }
    Box::leak(all.to_string().into_boxed_str())
}

// A rig whose App covers the repo, with `[triage]` set to `pick`, open issue
// 30 opened by `author` after triage was turned on, whose representative
// says WORDS and whose repo has the labels the pass puts on
fn filed(pick: &str, author: &str) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(&format!("pick = \"{pick}\""));
    rig.forge.set_repo_labels(&[
        "bug",
        "enhancement",
        "P1",
        "P2",
        "needs-triage",
        "needs-info",
        "ready-for-agent",
    ]);
    let runner = rig.open().unwrap();
    let mut crash = issue(&rig, 30, author, 5);
    crash.title = "Crash on start".to_owned();
    crash.body = "It crashes when I run it.".to_owned();
    list(&rig, crash);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    (rig, runner)
}

#[test]
fn a_maintainer_issue_it_is_sure_of_gets_the_new_body_its_labels_and_ready_for_agent_last() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    rig.claude.script_for(Role::Triager, [reply(sure())]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))]
    );
    // The body goes over the issue's as the reply wrote it, trimmed; the
    // title is untouched.
    assert_eq!(rig.github.issue_edits(), [(30, BODY.trim_end().to_owned())]);
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label ready-for-agent"]
    );
    let said = facts(&rig);
    assert!(said.contains("ready-for-agent"), "{said}");
    let triaged = triage(&rig, &runner);
    assert_eq!(triaged["issues"][0]["issue"], 30);
    assert_eq!(triaged["issues"][0]["step"], "ready");
    assert!(triaged["issues"][0].get("labels").is_none(), "{triaged}");
    assert!(
        triaged["issues"][0].get("ready_body").is_none(),
        "{triaged}"
    );
}

#[test]
fn an_outside_issue_it_is_sure_of_still_ends_needs_triage() {
    let (rig, runner) = filed("all", "newcomer");
    rig.claude.script_for(Role::Triager, [reply(sure())]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [],
        "only the maintainer's issues are rewritten"
    );
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"]
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
}

#[test]
fn a_body_with_no_criteria_or_no_touches_line_is_not_sure_and_ends_needs_triage() {
    let bodies = [
        // No acceptance criteria section.
        "Touches:\n- `src/start.rs`\n\n## What to build\n\n- Fix the crash.\n",
        // Criteria, but no `Touches:` line.
        "## What to build\n\n- Fix the crash.\n\n## Acceptance criteria\n\n- [ ] \
         `runner::start::it_starts`\n",
    ];
    for body in bodies {
        let (rig, runner) = filed("maintainer", MAINTAINER);
        let found = json!({ "type": "bug", "priority": "P2", "body": body });
        rig.claude.script_for(Role::Triager, [reply(found)]);

        assert_eq!(step(&runner).unwrap(), None, "{body}");

        assert_eq!(rig.github.issue_edits(), [], "{body}");
        assert_eq!(
            moves(&rig),
            ["comment", "label bug", "label P2", "label needs-triage"],
            "{body}"
        );
        assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done", "{body}");
    }
}

#[test]
fn a_reply_with_an_open_question_or_no_priority_is_not_sure() {
    let founds = [
        json!({
            "type": "bug", "priority": "P2", "body": BODY,
            "open": [{ "question": "Which platform?", "recommended": "Linux" }],
        }),
        json!({ "type": "bug", "body": BODY }),
    ];
    for found in founds {
        let (rig, runner) = filed("maintainer", MAINTAINER);
        rig.claude.script_for(Role::Triager, [reply(found.clone())]);

        assert_eq!(step(&runner).unwrap(), None, "{found}");

        assert_eq!(rig.github.issue_edits(), [], "{found}");
        assert_eq!(
            moves(&rig).last().map(String::as_str),
            Some("label needs-triage"),
            "{found}"
        );
        assert_eq!(
            triage(&rig, &runner)["issues"][0]["step"],
            "done",
            "{found}"
        );
    }
}

#[test]
fn a_body_naming_a_local_path_is_refused_and_the_issue_ends_needs_triage() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    let body = format!("Touches:\n- `/home/me/notes.md`\n\n{BODY}");
    rig.claude.script_for(
        Role::Triager,
        [reply(
            json!({ "type": "bug", "priority": "P2", "body": body }),
        )],
    );

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [],
        "the refused body never reaches the forge"
    );
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"]
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        told.iter()
            .any(|n| n.starts_with("triage #30:") && n.contains("needs-triage")),
        "{told:?}"
    );
}

#[test]
fn an_edit_the_forge_refused_is_told_once_and_tried_at_the_next_poll() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    rig.github.refuse_issue_edits(ApiError::Refused(403));
    rig.claude.script_for(Role::Triager, [reply(sure())]);

    assert_eq!(step(&runner).unwrap(), None);

    // The refused try was still sent; the record keeps the body owed.
    assert_eq!(rig.github.issue_edits().len(), 1);
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "commented");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    let mine: Vec<&String> = told
        .iter()
        .filter(|n| n.starts_with("triage #30:"))
        .collect();
    assert_eq!(mine.len(), 1, "told once: {told:?}");

    rig.github.accept_issue_edits();
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(moves(&rig).iter().filter(|m| **m == "comment").count(), 1);
    assert_eq!(
        rig.github.issue_edits().len(),
        2,
        "the same body, written again"
    );
    assert_eq!(
        moves(&rig).last().map(String::as_str),
        Some("label ready-for-agent")
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "ready");
}

#[test]
fn a_restart_after_the_body_edit_labels_it_without_editing_again() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    drop(runner);
    crashed_with(
        &rig,
        json!({ "issue": 30, "comment": 101, "step": "body-written", "labels": ["bug", "P2"] }),
    );
    let runner = rig.open().unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.claude.all_seen().len(), 0, "no call and no new wording");
    assert_eq!(rig.github.comments(), []);
    assert_eq!(
        rig.github.issue_edits(),
        [],
        "the body is not written twice"
    );
    assert_eq!(
        moves(&rig),
        ["label bug", "label P2", "label ready-for-agent"]
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "ready");
}

#[test]
fn a_restart_at_each_ready_step_goes_on_from_that_step() {
    let cases = [
        (
            json!({
                "issue": 30, "comment": 101, "step": "commented",
                "labels": ["bug", "P2"], "ready_body": BODY,
            }),
            vec![(30, BODY.to_owned())],
            vec!["label bug", "label P2", "label ready-for-agent"],
        ),
        (
            json!({ "issue": 30, "comment": 101, "step": "body-written", "labels": ["bug", "P2"] }),
            vec![],
            vec!["label bug", "label P2", "label ready-for-agent"],
        ),
        (
            json!({ "issue": 30, "comment": 101, "step": "ready" }),
            vec![],
            vec![],
        ),
    ];
    for (record, edits, expected) in cases {
        let (rig, runner) = filed("maintainer", MAINTAINER);
        drop(runner);
        crashed_with(&rig, record.clone());
        let runner = rig.open().unwrap();

        assert_eq!(step(&runner).unwrap(), None, "{record}");

        assert_eq!(rig.claude.all_seen().len(), 0, "{record}");
        assert_eq!(rig.github.comments(), [], "{record}");
        assert_eq!(rig.github.issue_edits(), edits, "{record}");
        assert_eq!(moves(&rig), expected, "{record}");
        assert_eq!(
            triage(&rig, &runner)["issues"][0]["step"],
            "ready",
            "{record}"
        );
    }
}

#[test]
fn a_triage_label_put_on_while_the_call_ran_stands_and_the_body_is_kept() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    let hold = Hold::default();
    rig.claude.script_for(
        Role::Triager,
        [Scripted::HoldThenSay(hold.clone(), said(sure()))],
    );

    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the research never began");
    rig.forge.label(30, "needs-info");
    hold.release();
    pump(&runner, || rig.github.comments().len() == 1);

    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))]
    );
    assert_eq!(
        rig.github.issue_edits(),
        [],
        "the body is kept as the author wrote it"
    );
    assert_eq!(moves(&rig), ["comment"], "nothing more is labelled");
    assert_eq!(rig.forge.issue_labels(30), ["needs-info"]);
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
}

#[test]
fn the_maintainer_login_matches_ignoring_case() {
    for author in ["boss", "BOSS", "Boss"] {
        let (rig, runner) = filed("all", author);
        rig.claude.script_for(Role::Triager, [reply(sure())]);

        assert_eq!(step(&runner).unwrap(), None, "{author}");

        assert_eq!(rig.github.issue_edits().len(), 1, "{author}");
        assert_eq!(
            moves(&rig).last().map(String::as_str),
            Some("label ready-for-agent"),
            "{author}"
        );
        assert_eq!(
            triage(&rig, &runner)["issues"][0]["step"],
            "ready",
            "{author}"
        );
    }
}

#[test]
fn a_ready_issue_is_recorded_ready_and_a_refused_edit_costs_no_second_call() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    rig.github.refuse_issue_edits(ApiError::Refused(502));
    rig.claude.script_for(Role::Triager, [reply(sure())]);
    assert_eq!(step(&runner).unwrap(), None);
    rig.github.accept_issue_edits();
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 1, "the pass ran once");
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "ready");
}
