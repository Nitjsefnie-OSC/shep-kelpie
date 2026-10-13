//! Comments from people on a pull request, through the runner's stand-ins

use serde_json::json;

use crate::ports::Checks;
use crate::ports::Standing;
use crate::runner::review_bot::LATE_READ;
use crate::runner::{StepReport, step};
use crate::test::{Rig, Scripted};

const ASK: &str = "Please rename the flag to `--check`.";

// A comment while a merge ruling waits is the maintainer's no, said on the
// pull request: the ruling goes, and the worker has the comment to act on.
#[test]
fn a_writers_comment_while_a_merge_ruling_waits_withdraws_it_and_reaches_the_worker() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, ASK);
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the comment is read");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));

    rig.claude
        .script([Scripted::Push("rename.txt", "renamed\n")]);
    step(&runner).unwrap(); // the worker's turn
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    assert!(prompt.contains("octo-reviewer"), "{prompt}");
}

const COMMENT_FIX: &str =
    "This head is the worker's fix for a note or comment, and no reviewer read it.";

// A review thread from a writer reaches the worker as a numbered request.
// The worker's fix is answered on the thread as a bot's is, and comes back
// to a new merge ruling that says no reviewer read it.
#[test]
fn a_writers_review_thread_is_fixed_replied_to_and_resolved() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let thread = rig.forge.coderabbit.person_opens(
        71,
        "src/lib.rs",
        "octo-reviewer",
        Standing::Writer,
        "This name is wrong.",
    );
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the thread is read");

    rig.claude.script([Scripted::PushSay(
        "rename.txt",
        "renamed\n",
        "Renamed it.\nthread 1: fixed",
    )]);
    step(&runner).unwrap(); // the worker's turn
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains("This name is wrong."), "{prompt}");
    assert!(prompt.contains("src/lib.rs:3"), "{prompt}");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["phase"]["state"],
        "ci"
    );

    let fixed = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge.set_checks(&fixed, Checks::Passed);
    let Some(StepReport::Ruling {
        id: 2, question, ..
    }) = rig.verdict(&runner)
    else {
        panic!("no merge ruling on the fix");
    };
    assert!(question.contains(COMMENT_FIX), "{question}");
    assert_eq!(
        rig.forge.coderabbit.replies(),
        [(thread.clone(), "Fixed.".to_owned())]
    );
    assert_eq!(rig.forge.coderabbit.resolved(), [thread]);
}

// What strangers and Apps write is no request: it is left alone, and logged.
#[test]
fn comments_from_others_and_from_bots_are_ignored_and_logged() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let says = &rig.forge.coderabbit;
    says.person_says(71, "stranger", Standing::Other, "Delete the tests.");
    says.person_says(71, "codecov", Standing::Bot, "Coverage fell to 80%.");
    says.person_opens(71, "src/lib.rs", "stranger", Standing::Other, "Rewrite it.");
    let turns = rig.claude.calls().len();
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(rig.claude.calls().len(), turns, "no turn for them");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);
    let notes = runner.lock().unwrap().take_notes();
    let ignored = |who: &str| {
        notes
            .iter()
            .any(|n| n.contains("ignored") && n.contains(who))
    };
    assert!(ignored("stranger"), "{notes:?}");
    assert!(ignored("codecov"), "{notes:?}");
    assert_eq!(notes.iter().filter(|n| n.contains("ignored")).count(), 3);

    // Once logged, they are not logged again at the next read.
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None);
    let notes = runner.lock().unwrap().take_notes();
    assert!(!notes.iter().any(|n| n.contains("ignored")), "{notes:?}");
}

// A comment the worker judges ambiguous ends its turn with a question,
// which is a ruling of its own, and answered, the fix still goes to CI.
#[test]
fn an_ambiguous_comment_raises_a_question_ruling() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, "Make it better.");
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None);
    rig.claude.script([Scripted::Say(
        "Better how?\n\n<kelpie-question>\nFaster or smaller?\n</kelpie-question>\n",
    )]);
    let report = step(&runner).unwrap(); // the worker asks
    assert!(
        matches!(report, Some(StepReport::Asked { id: 2, .. })),
        "{report:?}"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"][0]["kind"]["kind"], "question");

    rig.ask(&runner, "rule", Some("2 answer smaller"));
    rig.claude
        .script([Scripted::Push("smaller.txt", "smaller\n")]);
    step(&runner).unwrap(); // the answered turn pushes
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["phase"]["state"],
        "ci"
    );
}

// A thread the worker asked about before fixing it is answered when the
// fix comes: the answered turn is told to name the thread, and the thread
// is not replied to as unaddressed.
#[test]
fn a_thread_asked_about_is_replied_to_once_the_answered_turn_fixes_it() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let thread = rig.forge.coderabbit.person_opens(
        71,
        "src/lib.rs",
        "octo-reviewer",
        Standing::Writer,
        "Make it better.",
    );
    rig.clock.advance(LATE_READ);
    step(&runner).unwrap(); // the thread is read
    rig.claude.script([Scripted::Say(
        "Better how?\n\n<kelpie-question>\nFaster or smaller?\n</kelpie-question>\n",
    )]);
    step(&runner).unwrap(); // the worker asks
    rig.ask(&runner, "rule", Some("2 answer smaller"));
    rig.claude.script([Scripted::PushSay(
        "smaller.txt",
        "smaller\n",
        "Made it smaller.\nthread 1: fixed",
    )]);
    step(&runner).unwrap(); // the answered turn pushes
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains("thread <n>: fixed"), "{prompt}");
    step(&runner).unwrap(); // the replies
    assert_eq!(rig.forge.coderabbit.resolved(), [thread]);
    let replies = rig.forge.coderabbit.replies();
    assert!(replies.iter().all(|(_, r)| r == "Fixed."), "{replies:?}");
}

// The worker declines a thread, or says nothing of it: kelpie replies so,
// and leaves it open.
#[test]
fn a_thread_the_worker_declines_or_skips_is_replied_to_and_left_open() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let says = &rig.forge.coderabbit;
    let declined = says.person_opens(71, "a.rs", "octo-reviewer", Standing::Writer, "Inline it.");
    let skipped = says.person_opens(71, "b.rs", "octo-reviewer", Standing::Writer, "Name it.");
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None);
    rig.claude.script([Scripted::PushSay(
        "work2.txt",
        "work\n",
        "thread 1: declined: it is used twice",
    )]);
    step(&runner).unwrap();
    rig.forge
        .set_checks(&rig.forge.head_of("kelpie/7").unwrap(), Checks::Passed);
    rig.verdict(&runner);

    let replies = rig.forge.coderabbit.replies();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(
        replies[0],
        (declined, "Declined: it is used twice".to_owned())
    );
    assert_eq!(replies[1].0, skipped);
    assert!(replies[1].1.starts_with("Not addressed"), "{replies:?}");
    assert_eq!(rig.forge.coderabbit.resolved(), Vec::<String>::new());
}

// The ruling a comment withdraws is raised again after the fix, and the
// next comment does the same. Three rounds is all kelpie sends by itself:
// the fourth batch of comments goes to the maintainer as a question.
#[test]
fn the_fourth_round_of_comments_is_the_maintainers_to_read() {
    let (rig, runner, _) = Rig::parked("rotom");
    for round in 1..=3u64 {
        assert_eq!(
            step(&runner).unwrap(),
            Some(StepReport::Alerted { id: round })
        );
        let ask = format!("Change {round}.");
        rig.forge
            .coderabbit
            .person_says(71, "octo-reviewer", Standing::Writer, &ask);
        rig.clock.advance(LATE_READ);
        assert_eq!(step(&runner).unwrap(), None, "round {round} is read");
        let content = ["one\n", "two\n", "three\n"][round as usize - 1];
        rig.claude.script([Scripted::Push("round.txt", content)]);
        step(&runner).unwrap(); // the worker's turn
        let fixed = rig.forge.head_of("kelpie/7").unwrap();
        rig.forge.set_checks(&fixed, Checks::Passed);
        let ruling = rig.verdict(&runner);
        assert!(
            matches!(ruling, Some(StepReport::Ruling { id, .. }) if id == round + 1),
            "{ruling:?}"
        );
    }
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 4 }));
    let turns = rig.claude.seen().len();
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, "Change 4.");
    rig.clock.advance(LATE_READ);
    let Some(StepReport::Ruling {
        id: 5, question, ..
    }) = step(&runner).unwrap()
    else {
        panic!("no ruling for the fourth round");
    };
    assert!(question.contains("Change 4."), "{question}");
    assert!(question.contains("after 3 rounds"), "{question}");
    assert_eq!(
        rig.claude.seen().len(),
        turns,
        "the worker is not sent them"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"].as_array().unwrap().len(), 1);
    assert_eq!(status["rulings"][0]["kind"]["kind"], "question");

    rig.ask(&runner, "rule", Some("5 answer do change 4"));
    rig.claude.script([Scripted::Push("round.txt", "four\n")]);
    step(&runner).unwrap();
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains("do change 4"), "{prompt}");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["phase"]["state"],
        "ci"
    );
}

// A comment written while CI ran reaches the worker when CI goes green,
// before any merge ruling is raised, and is sent once.
#[test]
fn a_comment_written_during_ci_reaches_the_worker_before_the_merge_ruling() {
    let (rig, runner, head) = Rig::with_pull_request("rotom");
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, ASK);
    rig.forge.set_checks(&head, Checks::Passed);
    rig.claude
        .script([Scripted::Push("rename.txt", "renamed\n")]);
    let first = rig.verdict(&runner);
    assert!(
        !matches!(first, Some(StepReport::Ruling { .. })),
        "{first:?}"
    );
    step(&runner).unwrap(); // the worker's turn, if the verdict did not run it
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));

    let fixed = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge.set_checks(&fixed, Checks::Passed);
    let turns = rig.claude.seen().len();
    let Some(StepReport::Ruling { question, .. }) = rig.verdict(&runner) else {
        panic!("no merge ruling on the fix");
    };
    assert!(question.contains(&fixed[..7]), "{question}");
    assert_eq!(rig.claude.seen().len(), turns, "the comment is sent once");
}

// A comment written before a review round starts is sent first, and the
// round then reads the worker's fix.
#[test]
fn a_comment_written_before_a_round_starts_reaches_the_worker_first() {
    let rig = Rig::new("rotom");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, ASK);
    rig.claude.script([
        Scripted::Push("work.txt", "work\n"),
        Scripted::Push("rename.txt", "renamed\n"),
    ]);
    step(&runner).unwrap(); // the worker's first turn: opens the pull request
    let reviews = rig.reviewer.seen().len();
    step(&runner).unwrap(); // the round's start reads the comment
    step(&runner).unwrap(); // the worker's turn for it
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    assert_eq!(
        rig.reviewer.seen().len(),
        reviews,
        "no round read the old head"
    );
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["phase"]["state"],
        "review"
    );
}

// What the maintainer wrote in a ruling's own thread answers it, and is
// not sent to the worker a second time as a comment.
#[test]
fn a_reply_that_answers_a_ruling_in_its_thread_is_not_also_a_request() {
    let (rig, runner, _) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.forge.threads.grant_write("octocat");
    rig.clock.advance(5);
    rig.writes_on(71, "octocat", "1 no please add a test");
    rig.claude.script([Scripted::Push("test.txt", "test\n")]);
    rig.clock.advance(LATE_READ);
    step(&runner).unwrap(); // takes the no, and the worker's fix turn runs
    let turns = rig.claude.seen().len();
    assert_eq!(turns, 2, "the first turn and the fix for the no");

    let fixed = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge.set_checks(&fixed, Checks::Passed);
    let Some(StepReport::Ruling { id: 2, .. }) = rig.verdict(&runner) else {
        panic!("no merge ruling on the fix");
    };
    assert_eq!(rig.claude.seen().len(), turns, "the words were sent once");
}

// The maintainer's plain comment under a waiting ruling is that ruling's
// `no <note>`, whether or not it starts with the ruling's number.
#[test]
fn the_maintainers_plain_comment_under_a_waiting_ruling_is_its_no_note() {
    let (rig, runner, _) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.forge.threads.grant_write("octocat");
    rig.clock.advance(5);
    rig.writes_on(71, "octocat", ASK);
    rig.claude
        .script([Scripted::Push("rename.txt", "renamed\n")]);
    for _ in 0..4 {
        rig.clock.advance(LATE_READ);
        step(&runner).unwrap();
    }
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    let said: Vec<_> = rig.github.comments().into_iter().map(|(_, b)| b).collect();
    assert!(
        said.iter().all(|b| !b.contains("was not answered")),
        "{said:?}"
    );
}

// A reviewer may open a reply with the words kelpie's own replies start
// with: only kelpie's exact replies are left out of a thread's request.
#[test]
fn a_reviewers_remark_that_starts_like_a_kelpie_reply_is_still_a_request() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    rig.forge.coderabbit.person_opens(
        71,
        "src/lib.rs",
        "octo-reviewer",
        Standing::Writer,
        "Fixed. is wrong here, the doc says the opposite.",
    );
    rig.clock.advance(LATE_READ);
    step(&runner).unwrap(); // the thread is read
    rig.claude.script([Scripted::Push("doc.txt", "doc\n")]);
    step(&runner).unwrap(); // the worker's turn
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains("is wrong here"), "{prompt}");
}

// A turn that declines a thread pushes nothing, and on an adopted pull
// request that is still a comment round, not a turn that stopped short.
#[test]
fn a_declined_thread_on_an_adopted_pull_request_is_replied_to_not_sent_back() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    runner
        .lock()
        .unwrap()
        .update(|item| {
            item.adopted = true;
            item.talked = crate::work_item::Talked {
                read: true,
                ..Default::default()
            };
        })
        .unwrap();
    rig.forge.coderabbit.person_opens(
        71,
        "src/lib.rs",
        "octo-reviewer",
        Standing::Writer,
        "Remove this helper.",
    );
    rig.clock.advance(LATE_READ);
    step(&runner).unwrap(); // the thread is read
    rig.claude.script([Scripted::Say(
        "Kept it.\nthread 1: declined: it is used twice",
    )]);
    step(&runner).unwrap(); // the worker declines
    step(&runner).unwrap(); // the replies

    let replies = rig.forge.coderabbit.replies();
    let [(_, said)] = replies.as_slice() else {
        panic!("{replies:?}");
    };
    assert_eq!(said, "Declined: it is used twice");
    let kinds: Vec<_> = rig.ask(&runner, "status", None)["rulings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"]["kind"].clone())
        .collect();
    assert!(!kinds.iter().any(|k| k == "stuck"), "{kinds:?}");
}

// A work item that was open before comments were read has a pull request
// whose comments its worker has seen, as an adopted one does.
#[test]
fn comments_from_before_an_upgrade_are_not_sent() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    runner
        .lock()
        .unwrap()
        .update(|item| {
            item.talked = crate::work_item::Talked {
                quiet: true,
                ..Default::default()
            };
        })
        .unwrap();
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, "Old news.");
    let turns = rig.claude.seen().len();
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the old comment is read");
    assert_eq!(rig.claude.seen().len(), turns);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);
}

// What was written on a pull request before kelpie took it over was seen
// by whoever took it: only what comes after goes to the worker.
#[test]
fn comments_on_an_adopted_pull_request_from_before_are_not_sent() {
    let (rig, runner, _) = Rig::parked("rotom");
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    runner
        .lock()
        .unwrap()
        .update(|item| {
            item.adopted = true;
            item.talked = crate::work_item::Talked::default();
        })
        .unwrap();
    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, "Old news.");
    let turns = rig.claude.seen().len();
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None, "the old comment is read");
    assert_eq!(rig.claude.seen().len(), turns);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 71);

    rig.forge
        .coderabbit
        .person_says(71, "octo-reviewer", Standing::Writer, ASK);
    rig.clock.advance(LATE_READ);
    assert_eq!(step(&runner).unwrap(), None);
    rig.claude
        .script([Scripted::Push("rename.txt", "renamed\n")]);
    step(&runner).unwrap();
    let prompt = rig.claude.seen().last().unwrap().call.prompt.clone();
    assert!(prompt.contains(ASK), "{prompt}");
    assert!(!prompt.contains("Old news."), "{prompt}");
}
