//! The author's round, through the runner's stand-ins: the ask, the questions,
//! the answers, and how the wait ends

use std::sync::Mutex;

use serde_json::{Value, json};

use super::ask::{ASK_MARKER, QUESTIONS_MARKER};
use super::comment::MARKER;
use super::pick_tests::{MAINTAINER, issue, list, rig_with};
use super::research_tests::{calls, moves, reply, restart, triage};
use crate::github::ApiError;
use crate::ports::{Clock, Cooldown, Role};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

// What the representative says for the triage comment, and for the questions
pub(super) const ASK_WORDS: &str = "Thanks for the report. Reply `github` or `none`.";
pub(super) const Q_WORDS: &str = "A few things I still need from you.";
pub(super) const A_WORDS: &str = "That settles most of it.";

// What the reading answers, as the JSON it was asked for
pub(super) fn read(word: &str) -> Scripted {
    Scripted::Text(Box::leak(
        json!({ "reading": word }).to_string().into_boxed_str(),
    ))
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
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(ASK_WORDS)]);
    rig.claude.script_for(Role::Triager, [reply(found)]);
    before(&rig);
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

#[test]
fn a_maintainer_issue_with_an_open_question_asks_github_or_none_and_adds_no_label() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig), [format!("{ASK_WORDS}\n\n{MARKER}")]);
    assert_eq!(moves(&rig), ["comment"], "no label goes on while it waits");
    // The facts the comment was worded from end asking the author.
    let asked_facts = rig
        .claude
        .all_seen()
        .into_iter()
        .find(|s| s.call.role == Role::Representative)
        .expect("a wording call")
        .call
        .prompt;
    assert!(asked_facts.contains("`github`"), "{asked_facts}");
    assert!(asked_facts.contains("`none`"), "{asked_facts}");
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "commented");
    // The labels the pass owes wait in the record until the wait ends.
    assert_eq!(held["labels"], json!(["bug", "P2"]), "{held}");
    assert!(held["asked"]["at"].is_number(), "{held}");
    assert!(held.get("questions").is_none(), "{held}");
}

#[test]
fn an_outside_issue_is_asked_under_pick_all_and_ends_needs_triage_under_pick_maintainer() {
    let (rig, runner) = filed("pick = \"all\"", "newcomer", asked());
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert!(held["asked"]["at"].is_number(), "{held}");
    assert_eq!(moves(&rig), ["comment"], "asked, so no label yet");

    // Asked by hand under `pick = "maintainer"`, it is triaged and not asked.
    let (rig, runner) = filed("pick = \"maintainer\"", "newcomer", asked());
    rig.ask(&runner, "triage", Some("30"));
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert!(held.get("asked").is_none(), "{held}");
    assert_eq!(held["step"], "done");
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"],
        "an outside author is not asked"
    );
}

#[test]
fn none_adds_needs_triage_and_posts_nothing() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "none");
    rig.claude.script_for(Role::Representative, [read("none")]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(
        posted(&rig),
        [format!("{ASK_WORDS}\n\n{MARKER}")],
        "no post"
    );
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"]
    );
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("asked").is_none(), "{held}");
}

#[test]
fn silence_for_wait_days_counts_as_none() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    rig.clock.advance(Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(moves(&rig), ["comment"], "one day is not `wait_days`");

    rig.clock.advance(Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig), [format!("{ASK_WORDS}\n\n{MARKER}")]);
    assert_eq!(
        moves(&rig),
        ["comment", "label bug", "label P2", "label needs-triage"]
    );
    assert_eq!(
        rig.claude
            .all_seen()
            .into_iter()
            .filter(|s| s.call.role == Role::Representative)
            .count(),
        1,
        "nothing was there to read"
    );
}

#[test]
fn a_reply_that_is_none_of_them_gets_one_plain_ask_and_a_second_counts_as_none() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "what do you mean?");
    rig.claude.script_for(
        Role::Representative,
        [
            read("other"),
            Scripted::Say("Reply `github` or `none`, please."),
        ],
    );
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig).len(), 2);
    assert!(posted(&rig)[1].ends_with(ASK_MARKER), "{}", posted(&rig)[1]);
    let held = record(&rig, &runner);
    assert_eq!(held["asked"]["asks"], 2, "{held}");
    assert_eq!(moves(&rig), ["comment", "comment"], "still no label");

    wrote(&rig, MAINTAINER, "I still do not follow.");
    rig.claude.script_for(Role::Representative, [read("other")]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig).len(), 2, "it asks once again, not twice");
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
fn a_reply_by_anyone_but_the_author_is_not_read() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, "someone-else", "github");
    wrote(&rig, "kelpie-shep-pm[bot]", "none");
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig), [format!("{ASK_WORDS}\n\n{MARKER}")]);
    assert_eq!(moves(&rig), ["comment"], "the wait still holds");
    assert!(record(&rig, &runner)["asked"]["at"].is_number());
    assert_eq!(
        rig.claude
            .all_seen()
            .into_iter()
            .filter(|s| s.call.role == Role::Representative)
            .count(),
        1,
        "no reply was read"
    );
}

#[test]
fn github_posts_numbered_questions_with_recommended_answers_up_to_max_questions() {
    let (rig, runner) = filed(
        "pick = \"maintainer\"\nmax_questions = 2",
        MAINTAINER,
        asked_as(4),
    );
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "github");
    rig.claude.script_for(
        Role::Representative,
        [read("github"), Scripted::Say(Q_WORDS)],
    );
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let round = posted(&rig).remove(1);
    assert_eq!(
        round,
        format!(
            "{Q_WORDS}\n\n❓ Q1 - Which one, 1?\n\n➡️ the 1th\n\n---\n\n❓ Q2 - Which one, 2?\n\n➡️ \
             the 2th\n\n{QUESTIONS_MARKER}"
        )
    );
    let held = record(&rig, &runner);
    assert!(held.get("asked").is_none(), "{held}");
    assert!(held["questions"]["comment"].is_number(), "{held}");
    assert!(held["questions"]["at"].is_number(), "{held}");
    assert_eq!(moves(&rig), ["comment", "comment"], "no label yet");
}

#[test]
fn no_answers_for_wait_days_ends_needs_triage() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    wrote(&rig, MAINTAINER, "github");
    rig.claude.script_for(
        Role::Representative,
        [read("github"), Scripted::Say(Q_WORDS)],
    );
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    rig.clock.advance(2 * Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(posted(&rig).len(), 2, "no second triage comment");
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
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    let held = record(&rig, &runner);
    assert!(held.get("questions").is_none(), "{held}");
    assert_eq!(held["step"], "done");
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
    assert_eq!(posted(&rig), [format!("{ASK_WORDS}\n\n{MARKER}")]);
    assert_eq!(moves(&rig), ["comment"], "kelpie labels it no further");
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("asked").is_none(), "{held}");

    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    rig.forge.unlist_open_issue(30);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(posted(&rig), [format!("{ASK_WORDS}\n\n{MARKER}")]);
    assert_eq!(moves(&rig), ["comment"]);
    let held = record(&rig, &runner);
    assert_eq!(held["step"], "done");
    assert!(held.get("asked").is_none(), "{held}");
}

#[test]
fn a_restart_while_waiting_keeps_the_ask_and_its_clock() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 2", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    let at = record(&rig, &runner)["asked"]["at"].clone();

    let runner = restart(&rig, runner);
    assert_eq!(step(&runner).unwrap(), None);
    let held = record(&rig, &runner);
    assert_eq!(
        held["asked"]["at"], at,
        "its clock is the one it began with"
    );
    assert_eq!(held["step"], "commented");
    assert_eq!(moves(&rig), ["comment"], "a restart labels nothing early");

    rig.clock.advance(2 * Rig::DAY);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
}

#[test]
fn an_ask_kelpie_cannot_post_is_given_up_on_and_its_reply_is_not_read_again() {
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);

    wrote(&rig, MAINTAINER, "what do you mean?");
    rig.github.refuse_comments(ApiError::Refused(502));
    rig.claude.script_for(
        Role::Representative,
        [
            read("other"),
            Scripted::Say("Reply `github` or `none`, please."),
            Scripted::Say("Reply `github` or `none`, please."),
            Scripted::Say("Reply `github` or `none`, please."),
            Scripted::Say("Reply `github` or `none`, please."),
            Scripted::Say("Reply `github` or `none`, please."),
            read("other"),
            Scripted::Say("Reply `github` or `none`, please."),
            read("other"),
            Scripted::Say("Reply `github` or `none`, please."),
        ],
    );

    // Each poll is one failure, and five is where triage gives up on the issue.
    for _ in 0..5 {
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
    }
    let worded = calls(&rig, Role::Representative).len();
    let done = moves(&rig);
    assert_eq!(
        worded, 7,
        "the ask comment, the reply read once, the ask worded five times"
    );

    // The author keeps writing, and nothing is tried again until kelpie restarts.
    for _ in 0..2 {
        wrote(&rig, MAINTAINER, "still not following");
        rig.next_look();
        assert_eq!(step(&runner).unwrap(), None);
    }

    assert_eq!(
        calls(&rig, Role::Representative).len(),
        worded,
        "nothing worded"
    );
    assert_eq!(moves(&rig), done, "nothing posted");
    let held = record(&rig, &runner);
    assert!(held["asked"]["at"].is_number(), "{held}");
    assert!(
        !(done.iter().any(|m| m == "label needs-triage")),
        "{done:?}"
    );
}

// GitHub is too busy to take the questions, for longer than a read reaches
// back. The author's `github` is still there when it can go up.
#[test]
fn a_github_reply_is_still_read_after_the_forge_was_too_busy_to_post_the_questions() {
    let (rig, runner) = filed("pick = \"maintainer\"\nwait_days = 30", MAINTAINER, asked());
    assert_eq!(step(&runner).unwrap(), None);
    wrote(&rig, MAINTAINER, "github");
    rig.github.refuse_comments(ApiError::RateLimited(Cooldown {
        retry_after: Some(60),
        reset: None,
    }));
    rig.claude.script_for(
        Role::Representative,
        [
            read("github"),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
            Scripted::Say(Q_WORDS),
        ],
    );

    // Six polls is past the slack a read reaches back, and each post failed.
    for _ in 0..6 {
        rig.next_look();
        step(&runner).unwrap();
        assert_eq!(taken(&rig), 1, "only the ask went up");
    }

    rig.github.accept_comments();
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(taken(&rig), 2, "the questions went up at last");
    let held = record(&rig, &runner);
    assert!(held["questions"]["comment"].is_number(), "{held}");
    assert!(held.get("asked").is_none(), "{held}");
    assert_eq!(
        calls(&rig, Role::Representative).len(),
        9,
        "the triage comment, the reply read once, the questions worded seven times"
    );
}
