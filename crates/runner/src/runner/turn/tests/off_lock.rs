//! A turn's token and the issue its first prompt quotes are read with the lock let go

use super::*;
use crate::runner::flight::{Pass, advance};
use crate::test::Asked;

// The worker tokens the App was asked for so far
fn mints(rig: &Rig) -> usize {
    (rig.github.asked().iter())
        .filter(|asked| matches!(asked, Asked::AccessToken(..)))
        .count()
}

#[test]
fn a_trigger_is_answered_while_the_worker_token_is_minted() {
    let (rig, runner) = with_issue_7("koji");
    rig.with_app();
    rig.claude.script([Scripted::Say("done")]);
    let mint = rig.github.hold_next_worker_token();

    let (reply, pass) = rig.asked_while_held(&runner, mint, "tell", Some("hello"));

    assert!(
        reply.is_some(),
        "the trigger waited for the lock the mint held"
    );
    assert!(matches!(pass, Ok(Pass::Started)), "{pass:?}");
    // The call runs on its own thread, so its end is waited for before it is counted.
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { .. })
    ));
    assert_eq!(rig.claude.all_calls().len(), 1, "the turn started anyway");
}

#[test]
fn a_trigger_is_answered_while_the_issue_is_read_for_the_first_prompt() {
    let (rig, runner) = with_issue_7("koji");
    rig.claude.script([Scripted::Say("done")]);
    let read = rig.forge.hold_next_issue_read();

    let (reply, pass) = rig.asked_while_held(&runner, read, "tell", Some("hello"));

    assert!(
        reply.is_some(),
        "the trigger waited for the lock the read held"
    );
    assert!(matches!(pass, Ok(Pass::Started)), "{pass:?}");
    // The call runs on its own thread, so its end is waited for before it is counted.
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { .. })
    ));
    let [seen] = rig.claude.seen().try_into().unwrap();
    assert!(
        seen.call.prompt.contains("Body of #7."),
        "the prompt lost the issue the read found: {}",
        seen.call.prompt
    );
}

#[test]
fn a_trigger_is_answered_while_the_issue_comments_are_read() {
    let (rig, runner) = with_issue_7("koji");
    rig.claude.script([Scripted::Say("done")]);
    let read = rig.forge.hold_next_issue_comments_read();

    let (reply, pass) = rig.asked_while_held(&runner, read, "tell", Some("hello"));

    assert!(
        reply.is_some(),
        "the trigger waited for the lock the read held"
    );
    assert!(matches!(pass, Ok(Pass::Started)), "{pass:?}");
    // The call runs on its own thread, so its end is waited for before it is counted.
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { .. })
    ));
}

#[test]
fn no_token_is_minted_for_a_turn_that_does_not_start() {
    let (rig, runner) = with_issue_7("koji");
    rig.with_app();
    rig.ask(&runner, "drain", Some("pause"));
    let before = mints(&rig);

    let pass = advance(&runner);

    assert!(matches!(pass, Ok(Pass::Idle)), "{pass:?}");
    assert_eq!(mints(&rig), before, "a token was minted for no call");
    assert_eq!(rig.claude.all_calls(), [], "the worker started anyway");
}

#[test]
fn a_work_item_dropped_during_the_read_starts_no_call() {
    let (rig, runner) = with_issue_7("koji");
    let mint = rig.forge.hold_next_issue_read();

    let (reply, pass) = rig.asked_while_held(&runner, mint, "drop", Some("7"));

    assert!(
        reply.is_some(),
        "the trigger waited for the lock the mint held"
    );
    assert!(!matches!(pass, Ok(Pass::Started)), "{pass:?}");
    assert_eq!(
        rig.claude.all_calls(),
        [],
        "the dropped item's worker started"
    );
    assert!(
        rig.ask(&runner, "status", None)["work_items"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the item is still in flight"
    );
}

#[test]
fn a_read_that_fails_is_handled_as_the_inline_read_was() {
    let (rig, runner) = with_issue_7("koji");
    rig.forge.remove_issue(7);

    let report = step(&runner).unwrap();

    assert!(
        matches!(
            &report,
            Some(StepReport::Failed { issue, question, .. })
                if *issue == ticket(7) && question.contains("cannot read #7: gh failed: no issue #7")
        ),
        "{report:?}"
    );
    assert_eq!(rig.claude.all_calls(), [], "the worker started anyway");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["turn"]["state"], "failed");
}
