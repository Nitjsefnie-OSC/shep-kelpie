//! A wake's prompt and the notes kelpie writes for the wake it answered:
//! the board carried in the prompt, and one line of what the wake was for,
//! what kelpie did, and the answer's `why`

use super::{PICK_12, answered, pm_seen, step_until, with_pm};
use crate::runner::{StepReport, step};
use crate::test::{Rig, Scripted};

fn read(rig: &Rig, name: &str) -> String {
    std::fs::read_to_string(rig.paths().pm.join(name)).unwrap_or_default()
}

// The board's events section of a prompt that carries the board
fn events(prompt: &str) -> String {
    let at = ["## Recent events", "## Since your last wake"]
        .iter()
        .find_map(|heading| prompt.find(heading))
        .expect("the board in the prompt holds its events");
    let rest = &prompt[at + 3..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].to_owned()
}

#[test]
fn the_first_wake_carries_the_board_and_does_not_tell_it_to_read_it() {
    let (rig, runner) = with_pm("koji");
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude.script([Scripted::Say(PICK_12)]);
    answered(step(&runner).unwrap());

    let [wake] = pm_seen(&rig).try_into().unwrap();
    let prompt = &wake.call.prompt;
    let board = read(&rig, "board.md");
    assert!(board.contains("#12"), "{board}");
    assert!(
        prompt.contains(&board),
        "the prompt holds the board `board.md` holds: {prompt}"
    );
    assert!(prompt.contains("## The board"), "{prompt}");
    assert!(
        prompt.contains("`board.md` holds the same text"),
        "{prompt}"
    );
    assert!(!prompt.contains("Read `board.md`"), "{prompt}");
}

#[test]
fn a_resumed_wake_carries_the_board_with_only_the_events_since_its_last_answer() {
    let (rig, runner) = with_pm("koji");
    // Each read of the ready queue after the first is an event the board tells.
    rig.forge.list_ready(9, true);
    step(&runner).unwrap();
    rig.forge.list_ready(12, true);
    rig.clock.advance(61);
    step(&runner).unwrap();

    rig.ask(&runner, "tell", Some("first"));
    rig.claude.script([
        Scripted::Say("{\"pick\": null}"),
        Scripted::Say("{\"pick\": null}"),
    ]);
    answered(Some(step_until(&runner, |r| {
        matches!(r, StepReport::PmAnswered { .. })
    })));

    rig.forge.list_ready(15, true);
    rig.clock.advance(61);
    step(&runner).unwrap();
    rig.ask(&runner, "tell", Some("second"));
    answered(Some(step_until(&runner, |r| {
        matches!(r, StepReport::PmAnswered { .. })
    })));

    let [first, second] = pm_seen(&rig).try_into().unwrap();
    let before = events(&first.call.prompt);
    assert!(before.starts_with("Recent events"), "{before}");
    assert!(before.contains("#12 joined the ready queue"), "{before}");

    let after = events(&second.call.prompt);
    assert!(
        after.starts_with("Since your last wake"),
        "the board the resumed wake carries is the one written after its answer: {after}"
    );
    assert!(after.contains("#15 joined the ready queue"), "{after}");
    assert!(
        !after.contains("#12 joined the ready queue"),
        "its cursor holds the events it has read: {after}"
    );
    assert!(
        second
            .call
            .prompt
            .contains("`board.md` holds the same text"),
        "{}",
        &second.call.prompt
    );
}
