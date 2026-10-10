//! The replies kelpie posts on the bot threads a fix turn was sent, from
//! the worker's own report of what it did with each

use std::sync::Mutex;

use super::{now, phase, summoned};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

// CodeRabbit's review of the head opens a thread for each title, and both go
// to the worker.
fn sent(titles: &[&str]) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = summoned("shep");
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

// A reply the forge refuses is tried again at the next step, and a thread
// already replied to is not replied to twice.
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
