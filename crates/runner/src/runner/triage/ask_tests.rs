//! The author's round, through the runner's stand-ins: the ask, the questions,
//! the answers, and how the wait ends

use std::sync::Mutex;

use serde_json::{Value, json};

use super::comment::MARKER;
use super::pick_tests::{MAINTAINER, issue, list, rig_with};
use super::research_tests::{calls, crashed_with, moves, reply, restart, triage};
use crate::ports::{Clock, Role};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

// What the representative says for the triage comment that asks, and for the one that reads the answers
pub(super) const ASK_WORDS: &str =
    "Thanks for the report. Answer the questions below in one comment.";
pub(super) const A_WORDS: &str = "That settles most of it.";

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

// A rig whose App covers the repo, with `[triage]` set to `lines`, open issue
// 30 by `author`, whose representative words the triage comment ASK_WORDS.
// `found` is what its first triager reply gives.
pub(super) fn filed(lines: &str, author: &str, found: Value) -> (Rig, Mutex<Runner>) {
    filed_before(lines, author, found, |_| {})
}

// [`filed`], with the rig changed before the runner opens, for a project's own
// hook, which a running runner does not read again
pub(super) fn filed_before(
    lines: &str,
    author: &str,
    found: Value,
    before: impl FnOnce(&Rig),
) -> (Rig, Mutex<Runner>) {
    let rig = rig_with(lines);
    rig.forge.set_repo_labels(&REPO_LABELS);
    let mut crash = issue(&rig, 30, author, 5);
    crash.title = "Crash on start".to_owned();
    crash.body = "It crashes when I run it.".to_owned();
    list(&rig, crash);
    before(&rig);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(ASK_WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(found)]);
    let runner = rig.open().unwrap();
    (rig, runner)
}

// What the first pass found: a type, a priority, and one question only the
// author can answer
pub(super) fn asked() -> Value {
    json!({
        "type": "bug",
        "priority": "P2",
        "open": [{ "question": "Which OS crashes?", "recommended": "Linux" }],
    })
}

// The same with `n` open questions
fn asked_as(most: usize) -> Value {
    let open: Vec<Value> = (1..=most)
        .map(|n| json!({ "question": format!("Which one, {n}?"), "recommended": format!("the {n}th") }))
        .collect();
    json!({ "type": "bug", "priority": "P2", "open": open })
}

// What the second pass found: everything settled, with what is settled as its body
pub(super) fn settled() -> Value {
    json!({
        "type": "bug",
        "priority": "P2",
        "body": "## Settled\n\n- Linux, on start.\n",
    })
}

// The same, written as an agent-ready body, which is what makes a maintainer's
// own issue ready
pub(super) fn settled_ready() -> Value {
    json!({
        "type": "bug",
        "priority": "P2",
        "body": "Touches:\n- `src/start.rs`\n\n## What to build\n\n- Fix the crash on start, \
            on Linux.\n\n## Acceptance criteria\n\n- [ ] `runner::start::it_starts`\n",
    })
}

// The author's word on the issue, a little after the ask, as the forge lists it
pub(super) fn wrote(rig: &Rig, author: &str, body: &str) {
    rig.clock.advance(10);
    rig.forge.threads.write(30, author, body, rig.clock.now());
}

// The triage record `status` holds for issue 30
pub(super) fn record(rig: &Rig, runner: &Mutex<Runner>) -> Value {
    let held = triage(rig, runner);
    let issues = match held["issues"].as_array() {
        Some(a) => a.clone(),
        None => panic!("no records: {held}"),
    };
    issues
        .into_iter()
        .find(|t| t["issue"] == 30)
        .expect("issue 30")
}

// How many comments GitHub took on issue 30, the App's own included
fn taken(rig: &Rig) -> usize {
    rig.github.posted_on(30).len()
}

// What the App posted on issue 30, in order
pub(super) fn posted(rig: &Rig) -> Vec<String> {
    rig.github
        .comments()
        .into_iter()
        .map(|(_, body)| body)
        .collect()
}

// The questions as the first comment lists them, the grilling skill's shape
const ONE_QUESTION: &str = "❓ Q1 - Which OS crashes?\n\n➡️ Linux";

// The part of the comment inside its collapsed section
fn collapsed(comment: &str) -> &str {
    let (_, rest) = comment
        .split_once("<details>")
        .expect("a <details> section");
    rest.split_once("</details>").expect("a closed section").0
}

// How many of the representative's calls have run
fn worded(rig: &Rig) -> usize {
    calls(rig, Role::Representative).len()
}

// What the second triager call was asked, which holds the answers fenced
fn answers_prompt(rig: &Rig) -> String {
    let mut seen = calls(rig, Role::Triager);
    assert_eq!(seen.len(), 2, "the first pass and the second");
    seen.remove(1).call.prompt
}

#[test]
fn the_first_comment_asks_the_questions_and_holds_the_local_prompt_collapsed() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());

    assert_eq!(step(&runner).unwrap(), None);

    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(
        comment.starts_with(&format!("{ASK_WORDS}\n\n{ONE_QUESTION}\n\n<details>")),
        "{comment}"
    );
    assert!(comment.ends_with(MARKER), "{comment}");
    let prompt = collapsed(&comment);
    assert!(prompt.contains("/grill-with-docs Issue "), "{prompt}");
    assert!(prompt.contains("#30: Crash on start."), "{prompt}");
    assert!(prompt.contains("(recommended: Linux)"), "{prompt}");
    assert!(
        !comment.replace(prompt, "").contains("/grill-with-docs"),
        "the prompt is only inside the section"
    );
    assert_eq!(moves(&rig), ["comment"], "no label goes on while it waits");
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "commented");
    // The labels the pass owes wait in the record until the wait ends.
    assert_eq!(held["labels"], json!(["bug", "P2"]), "{held}");
    assert!(held["questions"]["comment"].is_number(), "{held}");
    assert!(held["questions"]["at"].is_number(), "{held}");
    assert!(held["local"]["body"].is_number(), "{held}");
    assert!(held.get("asked").is_none(), "{held}");
    // The facts the comment was worded from no longer ask for a word.
    let facts = calls(&rig, Role::Representative).remove(0).call.prompt;
    assert!(!facts.contains("`github`"), "{facts}");
    assert!(!facts.contains("`none`"), "{facts}");
}

#[test]
fn the_comment_lists_the_questions_up_to_max_questions_with_their_recommended_answers() {
    let (rig, runner) = filed(
        "pick = \"maintainer\"\nmax_questions = 2",
        MAINTAINER,
        asked_as(4),
    );

    assert_eq!(step(&runner).unwrap(), None);

    let [comment] = posted(&rig).try_into().expect("one comment");
    assert!(
        comment.starts_with(&format!(
            "{ASK_WORDS}\n\n❓ Q1 - Which one, 1?\n\n➡️ the 1th\n\n---\n\n❓ Q2 - Which one, 2?\n\n\
             ➡️ the 2th\n\n<details>"
        )),
        "{comment}"
    );
    assert!(!comment.contains("Which one, 3?"), "{comment}");
}

#[test]
fn an_outside_issue_is_asked_under_pick_all_and_ends_needs_triage_under_pick_maintainer() {
    let (rig, runner) = filed("pick = \"all\"", "newcomer", asked());
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert!(held["questions"]["at"].is_number(), "{held}");
    assert_eq!(moves(&rig), ["comment"], "asked, so no label yet");

    // Asked by hand under `pick = "maintainer"`, it is triaged and not asked.
    let (rig, runner) = filed("pick = \"maintainer\"", "newcomer", asked());
    rig.ask(&runner, "triage", Some("30"));
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert!(held.get("questions").is_none(), "{held}");
    assert!(held.get("local").is_none(), "{held}");
    assert_eq!(held["step"], "done");
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"],
        "an outside author is not asked"
    );
    assert!(!posted(&rig)[0].contains("<details>"), "{:?}", posted(&rig));
}

#[test]
fn a_comment_with_no_keyword_starts_the_second_pass_with_it_as_the_answers() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "Linux, it dies right on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let prompt = answers_prompt(&rig);
    assert!(
        prompt.contains("Linux, it dies right on start."),
        "{prompt}"
    );
    assert!(prompt.contains(ONE_QUESTION), "{prompt}");
    assert_eq!(posted(&rig).len(), 2, "the answers comment, and no other");
    assert_eq!(worded(&rig), 2, "no reply is read for a word");
    assert_eq!(record(&rig, &runner)["step"], "done");
}

#[test]
fn a_github_followed_by_answers_is_read_as_answers_and_no_question_is_posted_again() {
    let (rig, runner) = filed("pick = \"all\"", "newcomer", asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, "newcomer", "github\n\nQ1: Linux, on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let prompt = answers_prompt(&rig);
    assert!(
        prompt.contains("github\n\nQ1: Linux, on start."),
        "{prompt}"
    );
    let posted = posted(&rig);
    assert_eq!(posted.len(), 2, "{posted:?}");
    assert_eq!(posted[1], format!("{A_WORDS}\n\n{MARKER}"));
    assert!(!posted[1].contains("❓"), "no question is posted again");
    assert_eq!(
        rig.github.issue_edits(),
        [(
            30,
            "It crashes when I run it.\n\n## Settled\n\n- Linux, on start.".to_owned()
        )]
    );
    assert_eq!(record(&rig, &runner)["step"], "done");
}

#[test]
fn silence_for_wait_days_hands_the_issue_to_the_maintainer_and_asks_nothing_more() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    rig.clock.advance(Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(moves(&rig), ["comment"], "one day is not `wait_days`");

    rig.clock.advance(Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig).len(), 1, "no second ask");
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"]
    );
    assert_eq!(worded(&rig), 1, "nothing was there to read");
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("questions").is_none(), "{held}");
    assert!(held.get("local").is_none(), "{held}");
}

#[test]
fn a_reply_by_anyone_but_the_author_is_not_the_answers() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, "someone-else", "Linux");
    wrote(&rig, "kelpie-shep-pm[bot]", "Linux");
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig).len(), 1);
    assert_eq!(moves(&rig), ["comment"], "the wait still holds");
    assert!(record(&rig, &runner)["questions"]["at"].is_number());
    assert_eq!(calls(&rig, Role::Triager).len(), 1, "no second pass");
}

#[test]
fn a_triage_label_put_on_by_hand_or_a_closed_issue_ends_the_wait_with_no_post() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    rig.forge.label(30, "needs-info");
    let mut labelled = issue(&rig, 30, MAINTAINER, 5);
    labelled.title = "Crash on start".to_owned();
    labelled.body = "It crashes when I run it.".to_owned();
    labelled.labels = vec!["needs-info".to_owned()];
    list(&rig, labelled);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(taken(&rig), 1);
    assert_eq!(moves(&rig), ["comment"], "kelpie labels it no further");
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("questions").is_none(), "{held}");
    assert!(held.get("local").is_none(), "{held}");

    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    rig.forge.unlist_open_issue(30);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(taken(&rig), 1);
    assert_eq!(moves(&rig), ["comment"]);
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("questions").is_none(), "{held}");
}

#[test]
fn a_restart_while_waiting_keeps_the_questions_and_their_clock() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    let at = record(&rig, &runner)["questions"]["at"].clone();

    let runner = restart(&rig, runner);
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert_eq!(
        held["questions"]["at"], at,
        "its clock is the one it began with"
    );
    assert_eq!(held["step"], "commented");
    assert_eq!(moves(&rig), ["comment"], "a restart labels nothing early");

    rig.clock.advance(2 * Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(record(&rig, &runner)["step"], "done");
}

// A file saved while kelpie waited on the word `github`, `local` or `none`
// holds an `asked` and no questions: what the author writes after it is the
// answers to the triage comment it was saved with.
#[test]
fn a_wait_saved_on_the_old_ask_takes_the_authors_comment_as_the_answers() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    drop(runner);
    crashed_with(
        &rig,
        json!({
            "issue": 30, "comment": held["questions"]["comment"], "step": "commented",
            "labels": ["bug", "P2"], "seen": [],
            "asked": { "at": held["questions"]["at"] },
        }),
    );
    let runner = rig.open().unwrap();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(moves(&rig), ["comment"], "a restart labels nothing early");

    wrote(&rig, MAINTAINER, "Linux, on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let prompt = calls(&rig, Role::Triager).remove(1).call.prompt;
    assert!(prompt.contains("Linux, on start."), "{prompt}");
    assert!(prompt.contains(ONE_QUESTION), "{prompt}");
}
