//! The replies kelpie posts on the bot threads a fix turn was sent, from
//! the worker's own report of what it did with each

use std::sync::Mutex;

use super::{now, phase, summoned_with};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

// CodeRabbit's review of the head opens a thread for each title, and both go
// to the worker.
fn sent(titles: &[&str]) -> (Rig, Mutex<Runner>) {
    sent_with(titles, |_| {})
}

// As `sent`, `setup` having changed the project's settings before its runner opens.
fn sent_with(titles: &[&str], setup: impl FnOnce(&Rig)) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = summoned_with("shep", setup);
    rig.forge
        .coderabbit
        .review(71, &head, now(&rig) + 60, titles);
    rig.clock.advance(60);
    rig.threads_read(&runner);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::ReviewFindingsSent { .. })
    ));
    (rig, runner)
}

#[test]
fn each_thread_gets_the_workers_line_and_only_the_fixed_one_is_resolved() {
    let (rig, runner) = sent(&["Name the flag.", "Guard the index."]);
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        "Done.\nthread 1: fixed\nthread 2: declined: the caller checks the index",
    )]);
    step(&runner).unwrap(); // the fix turn
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));
    assert_eq!(
        rig.forge.coderabbit.replies(),
        [
            ("PRRT_71_0".to_owned(), "Fixed.".to_owned()),
            (
                "PRRT_71_1".to_owned(),
                "Declined: the caller checks the index".to_owned()
            ),
        ]
    );
    assert_eq!(rig.forge.coderabbit.resolved(), ["PRRT_71_0"]);
    assert_eq!(phase(&rig, &runner)["state"], "ci");
}

// The worker names a thread by the number its line in the findings file
// starts with, so a number read off the file and answered resolves exactly
// that thread, whatever header the file opens with.
#[test]
fn the_number_a_findings_line_starts_with_names_its_thread() {
    let (rig, runner) = sent(&["Name the flag.", "Guard the index."]);
    let file = std::fs::read_to_string(rig.build_7().join("review-findings.md")).unwrap();
    let number_of = |title: &str| -> String {
        let line = file.lines().find(|l| l.contains(title)).unwrap();
        let (number, _) = line
            .strip_prefix("thread ")
            .unwrap()
            .split_once('|')
            .unwrap();
        number.to_owned()
    };
    let guard = number_of("Guard the index.");
    assert_ne!(guard, number_of("Name the flag."));

    let said = format!("thread {guard}: fixed");
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        Box::leak(said.into_boxed_str()),
    )]);
    step(&runner).unwrap(); // the fix turn
    step(&runner).unwrap(); // the threads are answered
    assert_eq!(rig.forge.coderabbit.resolved(), ["PRRT_71_1"]);
    let replies = rig.forge.coderabbit.replies();
    assert_eq!(
        replies.iter().find(|(t, _)| t == "PRRT_71_1").unwrap().1,
        "Fixed."
    );
    assert!(
        replies
            .iter()
            .find(|(t, _)| t == "PRRT_71_0")
            .unwrap()
            .1
            .starts_with("Not addressed")
    );
}

// The guarded forge always refuses a text naming this machine's folders, so
// a decline's reason that does can never be posted: it is replied to without
// the reason, and the threads after it are answered all the same.
#[test]
fn a_reason_naming_a_local_path_is_not_posted_and_blocks_no_other_thread() {
    let (rig, runner) = sent(&["Name the flag.", "Guard the index."]);
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        "thread 1: declined: it lives in /Users/me/.kelpie/wt/koji/7/src/a.rs\nthread 2: fixed",
    )]);
    step(&runner).unwrap(); // the fix turn
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));
    assert_eq!(
        rig.forge.coderabbit.replies(),
        [
            (
                "PRRT_71_0".to_owned(),
                "Declined: the worker's reason names something on the machine kelpie runs on, \
                 so it is not posted."
                    .to_owned()
            ),
            ("PRRT_71_1".to_owned(), "Fixed.".to_owned()),
        ]
    );
    assert_eq!(rig.forge.coderabbit.resolved(), ["PRRT_71_1"]);
    assert_eq!(phase(&rig, &runner)["state"], "ci");
}

// A project's publish hook refuses the same text every time, so a decline's
// reason it refuses is replied to without the reason, and the thread after it
// is answered all the same.
#[test]
fn a_reason_a_publish_hook_refuses_is_not_posted_and_blocks_no_other_thread() {
    let (rig, runner) = sent_with(&["Name the flag.", "Guard the index."], |rig| {
        rig.forbid_word(&["publish"], "Acme");
    });
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        "thread 1: declined: Acme asked for the long name\nthread 2: fixed",
    )]);
    step(&runner).unwrap(); // the fix turn
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));
    assert_eq!(
        rig.forge.coderabbit.replies(),
        [
            (
                "PRRT_71_0".to_owned(),
                "Declined: the worker's reason was refused by a hook of the project's, so it is \
                 not posted."
                    .to_owned()
            ),
            ("PRRT_71_1".to_owned(), "Fixed.".to_owned()),
        ]
    );
    assert_eq!(phase(&rig, &runner)["state"], "ci");
}

// A reply the forge refuses is tried again at the next step, and a thread
// already replied to is not replied to twice by a running runner.
#[test]
fn a_reply_the_forge_refuses_is_tried_again_and_no_thread_is_replied_to_twice() {
    let (rig, runner) = sent(&["Name the flag.", "Guard the index."]);
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        "thread 1: fixed\nthread 2: declined: the caller checks the index",
    )]);
    step(&runner).unwrap(); // the fix turn
    rig.forge.coderabbit.set_reply_down(true);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::GateFailed { .. })
    ));
    assert_eq!(rig.forge.coderabbit.replies(), []);
    assert_eq!(phase(&rig, &runner)["stage"]["stage"], "fixing");

    rig.forge.coderabbit.set_reply_down(false);
    rig.next_look();
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));
    let replies: Vec<String> = (rig.forge.coderabbit.replies().into_iter())
        .map(|(thread, _)| thread)
        .collect();
    assert_eq!(replies, ["PRRT_71_0", "PRRT_71_1"]);
    assert_eq!(phase(&rig, &runner)["state"], "ci");
}

// A fixed thread is resolved before it is replied to, so a resolve the
// forge refuses holds its reply back, and the retry answers it once.
#[test]
fn a_thread_that_cannot_be_resolved_is_not_replied_to_until_it_is() {
    let (rig, runner) = sent(&["Name the flag.", "Guard the index."]);
    rig.claude.script([Scripted::PushSay(
        "flag.txt",
        "named\n",
        "thread 1: fixed\nthread 2: fixed",
    )]);
    step(&runner).unwrap(); // the fix turn
    rig.forge.coderabbit.set_resolve_down(true);
    rig.forge.coderabbit.set_reply_down(false);
    // Thread 1's resolve fails first, so nothing is replied to yet.
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::GateFailed { .. })
    ));
    assert_eq!(rig.forge.coderabbit.replies(), []);
    let item = runner.lock().unwrap().state.work_items[0].clone();
    assert_eq!(item.threads_sent, ["PRRT_71_0", "PRRT_71_1"]);
    assert_eq!(item.thread_answers.len(), 2);

    rig.forge.coderabbit.set_resolve_down(false);
    rig.next_look();
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));
    assert_eq!(rig.forge.coderabbit.replies().len(), 2);
    let item = runner.lock().unwrap().state.work_items[0].clone();
    assert_eq!((item.threads_sent.len(), item.thread_answers.len()), (0, 0));
}
