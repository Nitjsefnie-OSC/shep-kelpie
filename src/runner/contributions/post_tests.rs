//! What the representative posts of a local reviewer's pass, through the
//! runner's stand-ins

use std::sync::Mutex;

use super::pass_tests::{listing, poll_through, read_through, write};
use super::security_tests::{commit_on, contribute, contribution};
use crate::ports::{CLEAN, Finding, Role, Severity};
use crate::runner::coderabbit::tests::now;
use crate::runner::review_bot::contribution_turns::{FINDING, contributed, until_summoned};
use crate::runner::{Runner, step};
use crate::test::{REVIEW_ID, Rig, Scripted, ScriptedRound, SentReview};

const FILE: &str = "one\ntwo\nthree\n";

const WORDED: &str = r#"{"body": "Thanks for the push. Two things below.", "comments": ["The first line needs work.", "The third line needs work."]}"#;

const NOTE: &str =
    "The reviewer read your latest push and found nothing. It is with the maintainer now.";

fn finding(file: &str, line: u32, what: &str, why: &str) -> Finding {
    Finding::new(Severity::Medium, file, line, what, why)
}

// Pull request 30, whose pass is answered by `mine` with `findings`, and a
// representative that says `said` to each call, once each
fn read_with(findings: Vec<Finding>, said: &[&'static str]) -> (Rig, Mutex<Runner>, String) {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    rig.reviewer.script([ScriptedRound::Findings(findings)]);
    let said = said.iter().map(|s| Scripted::Say(s));
    rig.claude.script_for(Role::Representative, said);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", FILE));
    (rig, runner, head)
}

fn two_lines() -> Vec<Finding> {
    vec![
        finding("one.txt", 1, "first is wrong", "it breaks"),
        finding("one.txt", 3, "third is wrong", "it breaks too"),
    ]
}

// The calls the representative was made
fn worded(rig: &Rig) -> Vec<String> {
    let all = rig.claude.all_seen();
    (all.into_iter())
        .filter(|s| s.call.role == Role::Representative)
        .map(|s| s.call.prompt)
        .collect()
}

fn review_of(rig: &Rig) -> SentReview {
    let mut all = rig.github.reviews();
    assert_eq!(all.len(), 1, "one review: {all:?}");
    let (number, review) = all.remove(0);
    assert_eq!(number, 30);
    review
}

#[test]
fn findings_are_posted_as_one_comment_only_review_on_the_head_with_a_comment_per_line() {
    let (rig, runner, head) = read_with(two_lines(), &[WORDED]);

    let held = read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    let review = review_of(&rig);
    assert_eq!(review.event, "COMMENT");
    assert_eq!(review.head, head);
    assert_eq!(review.body, "Thanks for the push. Two things below.");
    assert_eq!(
        review.comments,
        [
            (
                "one.txt".to_owned(),
                1,
                "The first line needs work.".to_owned()
            ),
            (
                "one.txt".to_owned(),
                3,
                "The third line needs work.".to_owned()
            ),
        ]
    );
    assert_eq!(worded(&rig).len(), 1, "one call worded the whole review");
    assert_eq!(rig.github.comments(), [], "no separate comment");
    assert_eq!(rig.forge.comments(), [], "nothing as the maintainer");
    assert_eq!(held["pass"].get("review"), None);
    assert_eq!(contribution(&rig, &runner)["pass"]["review"], REVIEW_ID);
}

#[test]
fn a_finding_with_no_line_in_the_diff_goes_in_the_body() {
    let found = vec![
        finding("one.txt", 2, "second is wrong", "it breaks"),
        finding("one.txt", 40, "far is wrong", "no such line"),
        finding("gone.txt", 1, "gone is wrong", "no such file"),
        finding("one.txt", 0, "whole file is wrong", "no line"),
    ];
    let said = r#"{"body": "Body.", "comments": ["On the second."]}"#;
    let (rig, runner, head) = read_with(found, &[said]);

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    let review = review_of(&rig);
    assert_eq!(
        review.comments,
        [("one.txt".to_owned(), 2, "On the second.".to_owned())]
    );
    // The representative's body names none of them, and the review still does.
    assert!(review.body.starts_with("Body."), "{}", review.body);
    for aside in [
        "`one.txt:40`: far is wrong\n\nno such line",
        "`gone.txt:1`: gone is wrong\n\nno such file",
        "`one.txt`: whole file is wrong\n\nno line",
    ] {
        assert!(review.body.contains(aside), "{aside} in {}", review.body);
    }
    assert!(!review.body.contains("second is wrong"), "it has a comment");
    let asked = &worded(&rig)[0];
    assert!(asked.contains("second is wrong"), "{asked}");
}

#[test]
fn a_rejected_line_comment_retries_as_a_body_only_review() {
    let (rig, runner, head) = read_with(two_lines(), &[WORDED]);
    rig.github.refuse_line_comments();

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    let all = rig.github.reviews();
    assert_eq!(all.len(), 2, "the review, then once more: {all:?}");
    let (_, retry) = &all[1];
    assert_eq!(retry.comments, []);
    assert_eq!(retry.event, "COMMENT");
    assert_eq!(retry.head, head);
    assert!(
        retry.body.contains("Thanks for the push."),
        "{}",
        retry.body
    );
    for text in ["The first line needs work.", "The third line needs work."] {
        assert!(retry.body.contains(text), "{text} in {}", retry.body);
    }
    assert_eq!(contribution(&rig, &runner)["pass"]["review"], REVIEW_ID);
}

#[test]
fn the_review_is_posted_once_per_head_across_a_restart() {
    let (rig, runner, head) = read_with(two_lines(), &[WORDED, WORDED]);
    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);
    assert_eq!(rig.github.reviews().len(), 1);

    poll_through(&rig, &runner);
    let again = rig.open().unwrap();
    poll_through(&rig, &again);

    assert_eq!(rig.github.reviews().len(), 1, "not posted again");
    assert_eq!(worded(&rig).len(), 1, "not worded again");
    assert_eq!(contribution(&rig, &again)["pass"]["review"], REVIEW_ID);
}

#[test]
fn a_mismatched_comment_count_falls_back_to_the_findings_own_words() {
    let one_too_few = r#"{"body": "Body.", "comments": ["Only one."]}"#;
    let (rig, runner, head) = read_with(two_lines(), &[one_too_few]);

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    let review = review_of(&rig);
    assert_eq!(
        review.comments,
        [
            (
                "one.txt".to_owned(),
                1,
                "first is wrong\n\nit breaks".to_owned()
            ),
            (
                "one.txt".to_owned(),
                3,
                "third is wrong\n\nit breaks too".to_owned()
            ),
        ]
    );
    assert!(!review.body.is_empty());
    assert!(!review.body.contains("Only one."));
}

#[test]
fn an_answer_that_is_not_json_falls_back_too() {
    let (rig, runner, head) = read_with(two_lines(), &["Looks fine to me."]);

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    let review = review_of(&rig);
    assert_eq!(review.comments.len(), 2);
    assert_eq!(review.comments[0].2, "first is wrong\n\nit breaks");
}

#[test]
fn the_publish_hook_sees_the_body_and_each_line_comment() {
    const WORD: &str = "tangerine";
    let in_body = r#"{"body": "A tangerine.", "comments": ["One.", "Two."]}"#;
    let in_a_comment = r#"{"body": "Body.", "comments": ["One.", "A tangerine."]}"#;
    let (rig, _runner, head) = read_with(two_lines(), &[in_body, in_a_comment, WORDED]);
    rig.forbid_word(&["publish"], WORD);
    let runner = rig.open().unwrap();

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    // Neither the body nor the comment that named the word was posted; the
    // third wording had none and was.
    let review = review_of(&rig);
    assert_eq!(review.body, "Thanks for the push. Two things below.");
    assert_eq!(worded(&rig).len(), 3);
    assert_eq!(
        rig.github.reviews().len(),
        1,
        "a refused review is never sent"
    );
}

#[test]
fn nothing_is_posted_as_the_maintainer_when_the_app_cannot_post() {
    let (rig, runner, head) = read_with(two_lines(), &[WORDED; 6]);
    read_through(&rig, &runner, &head);
    rig.github.refuse_reviews();

    poll_through(&rig, &runner);

    assert_eq!(rig.forge.comments(), [], "never as the maintainer");
    assert_eq!(contribution(&rig, &runner)["pass"].get("review"), None);
    let tried = rig.github.reviews().len();
    assert!(tried >= 2, "tried at each poll: {tried}");
    let told = runner.lock().unwrap().take_notes();
    let told: Vec<_> = told.iter().filter(|n| n.contains("cannot post")).collect();
    assert_eq!(told.len(), 1, "told once: {told:?}");

    rig.github.accept_reviews();
    poll_through(&rig, &runner);

    assert_eq!(contribution(&rig, &runner)["pass"]["review"], REVIEW_ID);
    assert_eq!(rig.github.reviews().len(), tried + 1);
}

#[test]
fn a_bots_findings_are_not_posted() {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    let before = rig.github.comments();
    rig.forge
        .coderabbit
        .cubic_review(30, &head, now(&rig) + 30, &[FINDING]);

    let held = read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);

    assert_eq!(held["pass"]["answered"], "cubic");
    assert_eq!(rig.github.reviews(), []);
    assert_eq!(rig.github.comments(), before, "nothing but the summons");
    assert_eq!(worded(&rig), Vec::<String>::new());
}

#[test]
fn a_clean_pass_posts_one_short_note_once_per_head() {
    let (rig, runner, head) = read_with(vec![], &[NOTE, NOTE]);

    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);
    poll_through(&rig, &runner);

    assert_eq!(rig.github.comments(), [(30, NOTE.to_owned())]);
    assert_eq!(rig.github.reviews(), []);
    assert_eq!(rig.forge.comments(), []);
    assert_eq!(worded(&rig).len(), 1);
    let held = contribution(&rig, &runner);
    assert!(held["pass"]["note"].as_u64().is_some(), "{held}");

    // A restart does not say it again.
    let again = rig.open().unwrap();
    poll_through(&rig, &again);
    assert_eq!(rig.github.comments().len(), 1);
}

#[test]
fn a_note_that_could_not_be_worded_is_tried_at_the_next_poll() {
    let (rig, runner, head) = read_with(vec![], &[]);
    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);
    assert_eq!(rig.github.comments(), []);

    rig.claude
        .script_for(Role::Representative, [Scripted::Say(NOTE)]);
    poll_through(&rig, &runner);

    assert_eq!(rig.github.comments(), [(30, NOTE.to_owned())]);
}

#[test]
fn a_push_is_read_and_posted_again() {
    let (rig, runner, head) = read_with(vec![], &[NOTE, NOTE]);
    read_through(&rig, &runner, &head);
    poll_through(&rig, &runner);
    rig.claude.script([Scripted::Text(CLEAN)]);
    let second = commit_on(&rig, 30, "topic/30", |h| {
        write(h, "two.txt", "two\n");
    });

    read_through(&rig, &runner, &second);
    poll_through(&rig, &runner);

    assert_eq!(rig.github.comments().len(), 2, "one note a head");
}

#[test]
fn a_post_the_state_could_not_keep_is_not_posted_again() {
    use std::os::unix::fs::PermissionsExt;

    let (rig, runner, head) = read_with(two_lines(), &[WORDED; 4]);
    read_through(&rig, &runner, &head);
    let folder = rig.paths().state.parent().unwrap().to_owned();
    let open = std::fs::metadata(&folder).unwrap().permissions();
    let mut shut = open.clone();
    shut.set_mode(0o555);
    std::fs::set_permissions(&folder, shut).unwrap();

    let mut failed = 0;
    for _ in 0..3 {
        rig.next_look();
        failed += usize::from(step(&runner).is_err());
    }
    assert!(failed > 0, "the state could not be saved");
    std::fs::set_permissions(&folder, open).unwrap();
    poll_through(&rig, &runner);

    assert_eq!(rig.github.reviews().len(), 1, "the forge took it once");
    assert_eq!(contribution(&rig, &runner)["pass"]["review"], REVIEW_ID);
}
