use std::sync::Mutex;

use crate::ports::{Clock, Timestamp};
use crate::runner::{BOARD_POLL, CHECKS_SETTLE, Runner, step};
use crate::state::{Heard, Ruling, RulingKind, Stuck};
use crate::test::Rig;

const EXPECTS: &str = "it takes `yes`, `no <note>` or `rework <note>`";

// A merge ruling for pull request 71 is up through the App, which mentions
// `octocat`, who has write access to the repo.
fn a_merge_ruling_on_71() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.forge.threads.grant_write("octocat");
    // The ruling went up some seconds ago.
    rig.clock.advance(5);
    (rig, runner, head)
}

// The ids of the rulings that wait.
fn pending(rig: &Rig, runner: &Mutex<Runner>) -> Vec<u64> {
    let status = rig.ask(runner, "status", None);
    let rulings = status["rulings"].as_array().unwrap();
    rulings.iter().map(|r| r["id"].as_u64().unwrap()).collect()
}

// What kelpie posted on thread 71 after the ruling's own comment.
fn replies_on_71(rig: &Rig) -> Vec<String> {
    let comments = rig.github.comments();
    let after = comments
        .iter()
        .position(|(_, body)| body.starts_with("@octocat Merge"));
    let comments = &comments[after.unwrap() + 1..];
    comments
        .iter()
        .map(|(thread, body)| {
            assert_eq!(*thread, 71);
            body.clone()
        })
        .collect()
}

#[test]
fn a_yes_from_a_login_with_write_access_merges_the_pull_request() {
    let (rig, runner, head) = a_merge_ruling_on_71();
    let comment = rig.writes_on(71, "octocat", "yes");

    rig.next_look();
    step(&runner).unwrap(); // takes the reply, and marks the pull request ready
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap(); // merges

    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.github.reactions(), [comment], "kelpie took the reply");
}

#[test]
fn a_reply_from_a_login_without_write_access_changes_nothing() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.writes_on(71, "stranger", "yes");

    rig.next_look();
    step(&runner).unwrap();
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap();

    assert_eq!(rig.forge.merges(), []);
    assert_eq!(pending(&rig, &runner), [1]);
    assert!(rig.github.reactions().is_empty(), "no reaction");
    assert_eq!(replies_on_71(&rig), Vec::<String>::new(), "no answer");
    assert_eq!(
        rig.forge.threads.asked(),
        ["stranger"],
        "its access was asked"
    );
}

#[test]
fn a_bots_comment_is_left_alone_without_asking_what_it_may_do() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    let now = rig.clock.now();
    rig.forge
        .threads
        .bot_writes(71, "coderabbitai[bot]", "yes", now);

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(pending(&rig, &runner), [1]);
    assert_eq!(rig.forge.threads.asked(), Vec::<String>::new());
    assert_eq!(replies_on_71(&rig), Vec::<String>::new());
}

#[test]
fn a_comment_from_before_the_ruling_is_not_an_answer() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer("octocat");
    rig.forge.threads.grant_write("octocat");
    let before = rig.clock.now();
    rig.clock.advance(60);
    let (runner, _) = (rig.open().unwrap(), ());
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.forge.threads.write(71, "octocat", "yes", before);
    rig.claude.script([crate::test::Scripted::Say(
        "<kelpie-question>\nWhich flag?\n</kelpie-question>\n",
    )]);
    step(&runner).unwrap();
    assert_eq!(pending(&rig, &runner), [1]);

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(
        pending(&rig, &runner),
        [1],
        "the old comment answered nothing"
    );
    assert!(rig.github.reactions().is_empty());
}

#[test]
fn an_unreadable_reply_gets_one_reply_saying_what_kelpie_expects_and_the_ruling_stays_open() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.writes_on(71, "octocat", "looks good to me");

    rig.next_look();
    step(&runner).unwrap();

    let told = format!(
        "@octocat Ruling 1 was not answered: {EXPECTS}. Reply again in this thread, \
         starting with `1` if other rulings wait on it."
    );
    assert_eq!(replies_on_71(&rig), [told]);
    assert_eq!(pending(&rig, &runner), [1]);
    assert!(rig.github.reactions().is_empty(), "nothing was taken");

    // The same comment is not read again, however long the ruling waits.
    for _ in 0..3 {
        rig.next_look();
        step(&runner).unwrap();
    }
    assert_eq!(replies_on_71(&rig).len(), 1, "told once");
}

#[test]
fn a_no_with_a_note_goes_to_the_worker_and_a_question_takes_any_text() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    let comment = rig.writes_on(71, "octocat", "no please add a test");
    rig.claude.script([crate::test::Scripted::Say(
        "<kelpie-question>\nWhich flag?\n</kelpie-question>\n",
    )]);

    rig.next_look();
    step(&runner).unwrap(); // takes the no; the worker's fix turn asks

    assert_eq!(pending(&rig, &runner), [2], "the worker asked in its place");
    assert_eq!(rig.github.reactions(), [comment]);
    let answer = rig.writes_on(71, "octocat", "use --check");
    rig.claude.script([crate::test::Scripted::Say("Done.")]);
    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(pending(&rig, &runner), Vec::<u64>::new());
    assert_eq!(rig.github.reactions(), [comment, answer]);
}

#[test]
fn comment_reads_stay_within_one_per_board_poll_however_often_the_loop_wakes() {
    let (rig, runner, _) = a_merge_ruling_on_71();

    for _ in 0..5 {
        step(&runner).unwrap();
    }
    assert_eq!(rig.forge.threads.reads().len(), 1);

    rig.clock.advance(BOARD_POLL.as_secs() - 1);
    for _ in 0..5 {
        step(&runner).unwrap();
    }
    assert_eq!(
        rig.forge.threads.reads().len(),
        1,
        "a second short of the poll"
    );

    rig.clock.advance(1);
    for _ in 0..5 {
        step(&runner).unwrap();
    }
    assert_eq!(rig.forge.threads.reads().len(), 2, "a poll on");
}

#[test]
fn no_comments_are_read_where_no_ruling_waits_or_no_app_covers_the_repo() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(rig.forge.threads.reads().len(), 0, "no ruling waits");

    let (rig, runner, _) = Rig::parked("shep");
    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(rig.forge.threads.reads().len(), 0, "no App covers the repo");
}

#[test]
fn a_ruling_answered_at_the_terminal_first_is_told_the_reply_came_too_late() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.writes_on(71, "octocat", "yes");
    rig.ask(&runner, "rule", Some("1 no not yet"));
    rig.claude.script([crate::test::Scripted::Say(
        "<kelpie-question>\nWhich flag?\n</kelpie-question>\n",
    )]);

    rig.next_look();
    step(&runner).unwrap();

    assert!(rig.github.reactions().is_empty());
    // The worker's question, ruling 2, went up on the thread too.
    let told = "@octocat Ruling 1 is already answered.";
    assert!(replies_on_71(&rig).contains(&told.to_owned()));
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_ruling_answered_in_its_thread_is_already_answered_at_the_terminal() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.writes_on(71, "octocat", "no not yet");
    rig.claude.script([crate::test::Scripted::Say("Fixed.")]);
    rig.next_look();
    step(&runner).unwrap();

    let told = rig.ask(&runner, "rule", Some("1 yes"));

    assert_eq!(
        told["error"].as_str().unwrap(),
        "ruling 1 is already answered, or was never raised"
    );
}

// A second pending ruling on the same pull request, as an older kelpie or a
// second source would leave it.
fn with_a_second_ruling_on_71(runner: &Mutex<Runner>, rig: &Rig) {
    let mut runner = runner.lock().unwrap();
    let mut next = runner.state.clone();
    next.last_ruling = 2;
    next.rulings.push(Ruling {
        id: 2,
        issue: Some(7),
        question: "Is it closed?".into(),
        pull_request: Some(71),
        kind: RulingKind::from(Stuck::Closed),
        alerted: true,
    });
    next.heard.insert(2, Heard::at(rig.clock.now()));
    runner.save(next).unwrap();
}

#[test]
fn with_several_rulings_on_one_thread_a_reply_names_the_one_it_answers() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    with_a_second_ruling_on_71(&runner, &rig);
    rig.clock.advance(1);
    rig.writes_on(71, "octocat", "yes");

    rig.next_look();
    step(&runner).unwrap();

    let told = "@octocat Rulings 1, 2 wait on this thread, so start your reply with the id of \
                the one you answer, such as `1 yes`.";
    assert_eq!(replies_on_71(&rig), [told]);
    assert_eq!(pending(&rig, &runner), [1, 2]);
    assert!(rig.github.reactions().is_empty());

    rig.clock.advance(1);
    let named = rig.writes_on(71, "octocat", "2 yes");
    rig.claude.script([crate::test::Scripted::Say("Done.")]);
    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(rig.github.reactions(), [named]);
    assert_eq!(pending(&rig, &runner), [1], "ruling 2 was answered");
}

#[test]
fn a_ruling_saved_before_replies_in_a_thread_is_read_from_when_it_is_first_seen() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    {
        let mut runner = runner.lock().unwrap();
        let mut next = runner.state.clone();
        next.heard.clear();
        runner.save(next).unwrap();
    }
    rig.writes_on(71, "octocat", "yes");
    rig.forge.threads.write(71, "octocat", "yes", Timestamp(1));

    rig.next_look();
    step(&runner).unwrap();

    // The reply from before it was seen answers nothing.
    assert_eq!(pending(&rig, &runner), [1]);
    assert!(rig.github.reactions().is_empty());
}
