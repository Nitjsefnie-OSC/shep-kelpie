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
// 30 opened by `author` after triage was turned on, whose representative says
// WORDS and whose repo has `labels`. `before` runs what a test needs set
// before the runner opens.
fn filed(pick: &str, author: &str) -> (Rig, Mutex<Runner>) {
    filed_at(pick, author, &REPO_LABELS, |_| {})
}

fn filed_before(pick: &str, author: &str, before: impl FnOnce(&Rig)) -> (Rig, Mutex<Runner>) {
    filed_at(pick, author, &REPO_LABELS, before)
}

fn filed_at(
    pick: &str,
    author: &str,
    labels: &[&str],
    before: impl FnOnce(&Rig),
) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(&format!("pick = \"{pick}\""));
    rig.forge.set_repo_labels(labels);
    let mut crash = issue(&rig, 30, author, 5);
    crash.title = "Crash on start".to_owned();
    crash.body = "It crashes when I run it.".to_owned();
    list(&rig, crash);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    before(&rig);
    let runner = rig.open().unwrap();
    (rig, runner)
}

// The labels a repo may hold for the pass, named bare
const REPO_LABELS: [&str; 7] = [
    "bug",
    "enhancement",
    "P1",
    "P2",
    "needs-triage",
    "needs-info",
    "ready-for-agent",
];

// The same, named the way the board reads a priority: `priority: P2`
const BOARD_LABELS: [&str; 8] = [
    "bug",
    "enhancement",
    "priority: P1",
    "priority: P2",
    "needs-triage",
    "needs-info",
    "ready-for-agent",
    "wontfix",
];

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

    // The body is read against the local guard before the comment is worded,
    // so nothing is promised that kelpie will not do, and nothing is sent.
    let said = facts(&rig);
    assert!(!said.contains("ready-for-agent"), "{said}");
    assert!(said.contains("The maintainer triages it next"), "{said}");
    assert_eq!(
        rig.github.issue_edits(),
        [],
        "the refused body never reaches the forge"
    );
    assert_eq!(
        rig.github.comments(),
        [(30, format!("{WORDS}\n\n{MARKER}"))],
        "one comment, and no notice to undo a promise"
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

// A refusal the runner cannot read beforehand, from the project's own publish
// hook: the comment promised the rewrite, so the refusal is said on the issue
// rather than left there unkept.
#[test]
fn a_body_a_hook_refused_is_said_on_the_issue_and_the_issue_ends_needs_triage() {
    let (rig, runner) = filed_before("maintainer", MAINTAINER, |rig| {
        rig.forbid_word(&["publish"], "Unwriteable")
    });
    let body = format!("Touches:\n- `src/start.rs`\n\nUnwriteable notes.\n\n{BODY}");
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
    let posted = rig.github.comments();
    assert_eq!(posted.len(), 2, "{posted:?}");
    assert_eq!(posted[0], (30, format!("{WORDS}\n\n{MARKER}")));
    assert!(posted[1].1.contains("did not rewrite"), "{posted:?}");
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage"
        ],
        "the triage comment, the notice that the rewrite was refused, then the labels"
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
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
    // The stand is read before the comment is worded, so the comment promises
    // nothing kelpie will not do.
    let said = facts(&rig);
    assert!(!said.contains("ready-for-agent"), "{said}");
    assert!(said.contains("The maintainer triages it next"), "{said}");
    assert_eq!(moves(&rig), ["comment"], "nothing more is labelled");
    assert_eq!(rig.forge.issue_labels(30), ["needs-info"]);
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
}

// The same stand, read again where the ready path's labels would go on: a
// label put on after the body went on is not labelled over.
#[test]
fn a_triage_label_put_on_after_the_body_edit_stands_and_ready_for_agent_is_not_put_on() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    drop(runner);
    crashed_with(
        &rig,
        json!({ "issue": 30, "comment": 101, "step": "body-written", "labels": ["bug", "P2"] }),
    );
    let runner = rig.open().unwrap();
    rig.forge.label(30, "ready-for-human");

    assert_eq!(step(&runner).unwrap(), None);

    let none: [String; 0] = [];
    assert_eq!(
        moves(&rig),
        none,
        "nothing is labelled over the maintainer's"
    );
    assert_eq!(rig.github.comments(), [], "no comment is posted again");
    assert_eq!(rig.forge.issue_labels(30), ["ready-for-human"]);
    let triaged = triage(&rig, &runner);
    assert_eq!(triaged["issues"][0]["step"], "done");
    assert!(triaged["issues"][0].get("labels").is_none(), "{triaged}");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        told.iter()
            .any(|n| n.contains("ready-for-human") && n.contains("stands")),
        "{told:?}"
    );
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

// The board ranks a ready issue by `priority: P2`, not by a bare `P2`. A repo
// named that way gets the label the board reads.
#[test]
fn a_board_named_priority_label_is_the_one_that_goes_on_a_readied_issue() {
    let (rig, runner) = filed_at("maintainer", MAINTAINER, &BOARD_LABELS, |_| {});
    rig.claude.script_for(Role::Triager, [reply(sure())]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        moves(&rig),
        [
            "comment",
            "label bug",
            "label priority: P2",
            "label ready-for-agent"
        ],
        "the triager's `P2` goes on under the name the board ranks by"
    );
}

// The same on the plain `needs-triage` path, which takes its labels from the
// same reading of the repo
#[test]
fn a_board_named_priority_label_is_the_one_that_goes_on_an_issue_that_ends_needs_triage() {
    let (rig, runner) = filed_at("all", "newcomer", &BOARD_LABELS, |_| {});
    rig.claude.script_for(Role::Triager, [reply(sure())]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        moves(&rig),
        [
            "comment",
            "label bug",
            "label priority: P2",
            "label needs-triage"
        ]
    );
}

// `shep kelpie triage` on an issue an earlier pass left at `needs-triage`, now
// sure of it: the issue is not left both ready for the board and waiting for
// the maintainer
#[test]
fn an_issue_triaged_again_and_now_sure_loses_the_needs_triage_of_the_first_pass() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    let asked = json!({
        "type": "bug",
        "priority": "P2",
        "open": [{ "question": "Is it every start?", "recommended": "Yes" }],
    });
    rig.claude
        .script_for(Role::Triager, [reply(asked), reply(sure())]);
    rig.claude.script_for(
        Role::Representative,
        [Scripted::Say(WORDS), Scripted::Say(WORDS)],
    );

    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"],
        "the first pass is not sure of it, so it ends `needs-triage`"
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");

    // The issue as the first pass left it, on the forge as on the board's
    // list, asked again
    for label in ["bug", "P2", "needs-triage"] {
        rig.forge.label(30, label);
    }
    let mut left = issue(&rig, 30, MAINTAINER, 5);
    left.title = "Crash on start".to_owned();
    left.labels = vec!["bug".to_owned(), "P2".to_owned(), "needs-triage".to_owned()];
    list(&rig, left);
    let reply = rig.ask(&runner, "triage", Some("30"));
    assert_eq!(reply.get("error"), None, "{reply}");
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [(30, BODY.trim_end().to_owned())],
        "the second pass rewrote it"
    );
    assert_eq!(
        rig.github.label_takes(),
        [(30, "needs-triage".to_owned())],
        "`needs-triage` came off before `ready-for-agent` went on"
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "label bug",
            "label P2",
            "label needs-triage",
            "comment",
            "label bug",
            "label P2",
            "label ready-for-agent",
        ]
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "ready");
}

// A pass that resumes after a restart has no in-memory reading of the labels
// the issue began with, so its record keeps them. Without that, the
// `needs-triage` an earlier pass put on reads as one the maintainer put on.
#[test]
fn a_restarted_pass_knows_its_own_needs_triage_from_the_record() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    drop(runner);
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": 101, "step": "commented", "labels": ["bug", "P2"],
            "ready_body": BODY, "seen": ["bug", "P2", "needs-triage"],
        }),
    );
    for label in ["bug", "P2", "needs-triage"] {
        rig.forge.label(30, label);
    }
    let runner = rig.open().unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [(30, BODY.to_owned())],
        "the owed rewrite goes on, as the comment promised"
    );
    assert_eq!(rig.github.label_takes(), [(30, "needs-triage".to_owned())]);
    assert_eq!(
        moves(&rig),
        ["label bug", "label P2", "label ready-for-agent"]
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "ready");
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        !told.iter().any(|n| n.contains("stands")),
        "{told:?}: kelpie's own label is not a hand label"
    );
}

// The same at the labelling step, where `ready-for-agent` is the label kelpie
// is writing. An answer lost after it went on, or a crash before the step
// saved, leaves it on the issue with the record still at `body-written`.
#[test]
fn a_ready_for_agent_already_on_the_issue_is_not_read_as_a_label_put_by_hand() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    drop(runner);
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": 101, "step": "body-written", "labels": ["bug", "P2"],
            "seen": ["needs-triage"],
        }),
    );
    rig.forge.label(30, "needs-triage");
    rig.forge.label(30, "ready-for-agent");
    let runner = rig.open().unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        moves(&rig),
        ["label bug", "label P2", "label ready-for-agent"]
    );
    assert_eq!(rig.github.label_takes(), [(30, "needs-triage".to_owned())]);
    assert_eq!(
        triage(&rig, &runner)["issues"][0]["step"],
        "ready",
        "the record says what the issue really is"
    );
    let told: Vec<String> = runner.lock().unwrap().take_notes();
    assert!(
        !told.iter().any(|n| n.contains("stands")),
        "{told:?}: the label this step writes is kelpie's own"
    );
}

// A label that is not the one this step writes still stands at that step
#[test]
fn a_label_put_by_hand_at_the_labelling_step_stands_though_ready_for_agent_is_owed() {
    let (rig, runner) = filed("maintainer", MAINTAINER);
    drop(runner);
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": 101, "step": "body-written", "labels": ["bug", "P2"],
            "seen": [],
        }),
    );
    rig.forge.label(30, "ready-for-agent");
    rig.forge.label(30, "ready-for-human");
    let runner = rig.open().unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        moves(&rig),
        Vec::<String>::new(),
        "kelpie labels nothing over `ready-for-human`"
    );
    assert_eq!(triage(&rig, &runner)["issues"][0]["step"], "done");
}
