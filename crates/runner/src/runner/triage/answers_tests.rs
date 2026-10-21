//! The answers half of the author's round, through the runner's stand-ins: the
//! second triager call, what it writes, and what it is not made or written from

use std::sync::Mutex;

use serde_json::{Value, json};

use super::ask_tests::{
    A_WORDS, asked, filed, filed_before, posted, record, settled, settled_ready, wrote,
};
use super::comment::MARKER;
use super::pick_tests::{MAINTAINER, issue, list};
use super::research_tests::{calls, crashed_with, moves, nothing, pump, reply, restart};
use crate::ports::Role;
use crate::runner::{Pass, Runner, advance, step};
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

#[test]
fn answers_settling_everything_on_a_maintainer_issue_make_it_ready_for_agent() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

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
    assert_eq!(
        rig.github.issue_edits(),
        [(30, body)],
        "the maintainer's own issue is rewritten over"
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label ready-for-agent"
        ]
    );
    assert_eq!(calls(&rig, Role::Triager).len(), 2);
    assert_eq!(record(&rig, &runner)["step"], "ready");
}

#[test]
fn answers_on_an_outside_issue_go_under_the_authors_text_and_end_needs_triage() {
    let (rig, runner) = filed("pick = \"all\"", "newcomer", asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, "newcomer", "Q1: Linux, on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [(
            30,
            "It crashes when I run it.\n\n## Settled\n\n- Linux, on start.".to_owned()
        )],
        "under the author's text, never in place of it"
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage"
        ]
    );
    assert_eq!(record(&rig, &runner)["step"], "done");
}

#[test]
fn a_question_still_open_after_the_answers_ends_needs_triage_with_no_second_round() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "Q1: Linux.");
    rig.claude.script_for(Role::Triager, [reply(asked())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        calls(&rig, Role::Triager).len(),
        2,
        "one round of questions is the whole of it"
    );
    assert_eq!(posted(&rig).len(), 2, "no second questions comment");
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage"
        ]
    );
    // The comment that ends the round is worded from what is still open.
    let mut worded = calls(&rig, Role::Representative);
    let last = worded.remove(worded.len() - 1).call.prompt;
    assert!(last.contains("Which OS crashes?"), "{last}");
    assert!(last.contains("last round"), "{last}");
    assert!(posted(&rig).last().unwrap().ends_with(MARKER));
}

#[test]
fn a_restart_while_waiting_on_the_answers_keeps_the_questions_and_their_clock() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    let (comment, at) = (
        held["questions"]["comment"].clone(),
        held["questions"]["at"].clone(),
    );

    let runner = restart(&rig, runner);
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert_eq!(held["questions"]["comment"], comment, "{held}");
    assert_eq!(
        held["questions"]["at"], at,
        "its clock is the one it began with"
    );
    assert_eq!(moves(&rig), ["comment"], "a restart labels nothing early");

    // The author's answer after the restart is still heard, and the second
    // pass runs.
    wrote(&rig, MAINTAINER, "Q1: Linux, on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 2);
    assert_eq!(
        rig.github.issue_edits(),
        [(
            30,
            "It crashes when I run it.\n\n## Settled\n\n- Linux, on start.".to_owned()
        )]
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage"
        ]
    );
}

#[test]
fn a_restart_at_the_answers_step_writes_the_body_once_and_labels_it() {
    let (rig, _) = filed("pick = \"all\"", "newcomer", asked());
    let body = "It crashes when I run it.\n\n## Settled\n\n- Linux, on start.";
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": 100, "step": "answers",
            "labels": ["bug", "P2"], "ready_body": body, "seen": [],
        }),
    );
    let runner = rig.open().unwrap();

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.github.issue_edits(), [(30, body.to_owned())]);
    assert_eq!(moves(&rig), ["label bug", "label P2", "label needs-triage"]);
    assert_eq!(rig.claude.all_seen().len(), 0, "no call and no new wording");
    assert_eq!(record(&rig, &runner)["step"], "done");
}

// The same reply as leaked text, for a scripted call held open
fn said(found: Value) -> &'static str {
    let mut all = nothing();
    for (key, value) in found.as_object().expect("an object") {
        all[key] = value.clone();
    }
    Box::leak(all.to_string().into_boxed_str())
}

#[test]
fn the_answers_call_is_not_made_when_its_questions_cannot_be_read() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    let mut lost = record(&rig, &runner)["questions"].clone();
    assert!(lost["comment"].is_number(), "{lost}");

    // The author answers, and the crash loses the questions comment the record
    // names and the questions the pass held.
    wrote(&rig, MAINTAINER, "Q1: Linux, on start.");
    drop(runner);
    lost["comment"] = json!(4242);
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": 100, "step": "commented",
            "labels": ["bug", "P2"], "seen": [], "questions": lost,
        }),
    );
    let runner = rig.open().unwrap();
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);

    for _ in 0..3 {
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
    }

    assert_eq!(
        calls(&rig, Role::Triager).len(),
        1,
        "no second call without the questions and answers it reads"
    );
    assert_eq!(
        rig.github.issue_edits(),
        [],
        "nothing is written into the body from what is not there"
    );
    assert_eq!(posted(&rig).len(), 1, "no answers comment");
    assert_eq!(moves(&rig), ["comment"], "the wait still holds");
    assert_eq!(record(&rig, &runner)["questions"]["comment"], 4242);
}

#[test]
fn a_hand_label_while_the_answers_call_runs_is_not_settled_under_it() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "Q1: Linux, on start.");
    let hold = Hold::default();
    rig.claude.script_for(
        Role::Triager,
        [Scripted::HoldThenSay(hold.clone(), said(settled()))],
    );
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);

    rig.next_look();
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the research never began");
    rig.forge.label(30, "ready-for-human");
    let mut stood = issue(&rig, 30, MAINTAINER, 5);
    stood.title = "Crash on start".to_owned();
    stood.body = "It crashes when I run it.".to_owned();
    stood.labels = vec!["ready-for-human".to_owned()];
    list(&rig, stood);

    // A poll goes by while the call is in flight. The wait is not ended under
    // it, since the call would then post onto a record already saved finished.
    rig.next_look();
    advance(&runner).unwrap();
    let held = record(&rig, &runner);
    assert!(held["questions"].is_object(), "{held}");
    assert_eq!(moves(&rig), ["comment"], "no label goes on early");

    hold.release();
    pump(&runner, || rig.github.comments().len() == 2);

    assert_eq!(posted(&rig).len(), 2);
    assert_eq!(rig.forge.issue_labels(30), ["ready-for-human"]);
    assert_eq!(
        moves(&rig),
        ["comment", "comment"],
        "the hand label stands and kelpie labels no further"
    );
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done", "{held}");
    assert!(
        held.get("asked").is_none() && held.get("questions").is_none(),
        "{held}"
    );
}

// The answers pass on an outside issue, up to its comment: the ask went up, the
// author said `github`, the questions went up and they answered them.
pub(super) fn asked_and_answered(rig: &Rig, runner: &Mutex<Runner>, author: &str, answers: &str) {
    assert_eq!(step(runner).unwrap(), None);
    wrote(rig, author, answers);
}
// A settled body a project's own publish hook refuses
#[test]
fn answers_a_hook_refused_still_put_the_type_and_priority_on() {
    let (rig, runner) = filed_before("pick = \"all\"", "newcomer", asked(), |rig| {
        rig.forbid_word(&["publish"], "Unwriteable")
    });
    asked_and_answered(&rig, &runner, "newcomer", "Q1: Linux, on start.");
    rig.claude.script_for(
        Role::Triager,
        [reply(json!({
            "type": "bug",
            "priority": "P2",
            "body": "## Settled\n\n- Linux, on start. Unwriteable notes.",
        }))],
    );
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);

    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        rig.github.issue_edits(),
        [],
        "the refused body never reaches the forge"
    );
    let posted = posted(&rig);
    assert_eq!(posted.len(), 3, "{posted:?}");
    assert!(
        posted[2].contains("did not write the answers"),
        "{posted:?}"
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage",
        ],
        "the type and priority the pass found are not lost with the body"
    );
    assert_eq!(record(&rig, &runner)["step"], "done");
}

// The issue's body cannot be read, so the pass has no settled body to put under
// the author's text. That is not stood for by a comment and a label.
#[test]
fn answers_whose_issue_body_cannot_be_read_post_nothing_and_are_tried_again() {
    let (rig, runner) = filed("pick = \"all\"", "newcomer", asked());
    asked_and_answered(&rig, &runner, "newcomer", "Q1: Linux, on start.");
    rig.claude
        .script_for(Role::Triager, [reply(settled()), reply(settled())]);
    rig.claude.script_for(
        Role::Representative,
        [Scripted::Say(A_WORDS), Scripted::Say(A_WORDS)],
    );
    // The pass reads its labels, then cannot read its body.
    rig.forge.fail_issue_reads_after(30, 1);

    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    let posted = posted(&rig);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(moves(&rig), ["comment"], "no label goes on");
    let held = record(&rig, &runner);
    assert!(held["questions"]["comment"].is_number(), "{held}");
    assert_eq!(held["step"], "commented", "the round is still open");

    rig.forge.let_issue_read(30);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(
        rig.github.issue_edits(),
        [(
            30,
            "It crashes when I run it.\n\n## Settled\n\n- Linux, on start.".to_owned()
        )],
        "under the author's text, once the body can be read"
    );
    assert_eq!(
        moves(&rig),
        [
            "comment",
            "comment",
            "label bug",
            "label P2",
            "label needs-triage",
        ]
    );
    assert_eq!(record(&rig, &runner)["step"], "done");
}
