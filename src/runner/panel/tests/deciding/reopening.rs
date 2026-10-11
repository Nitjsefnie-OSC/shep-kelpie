//! A `no <note>` reply to a ruling the panel decided reopens it

use super::{PANEL, ruling, script, voted};
use crate::runner::{CHECKS_SETTLE, step};
use crate::settings::Merging;

#[test]
fn a_no_note_reply_to_a_decided_ruling_sends_the_worker_the_note() {
    let (rig, runner) = ruling("reopen", "feat: add it", Merging::Auto, PANEL);
    rig.forge.threads.grant_write("octocat");
    script(&rig, ["A", "A", "A", "A"]);
    voted(&rig, &runner);
    assert!(
        super::decided_comment(&rig).is_some(),
        "the panel decided it first"
    );
    rig.clock.advance(5);

    let reply = rig.writes_on(71, "octocat", "1 no add a test for the empty case");
    rig.claude.script([crate::test::Scripted::Say(
        "<kelpie-question>\nWhich?\n</kelpie-question>\n",
    )]);
    rig.next_look();
    step(&runner).unwrap(); // reads the reply, reopens the ruling, and the worker takes its note

    assert_eq!(rig.github.reactions(), [reply], "kelpie took the reply");
    let calls = rig.claude.calls();
    let note = "add a test for the empty case";
    assert!(
        calls.iter().any(|c| c.prompt.contains(note)),
        "the worker was never sent the note"
    );
}

#[test]
fn a_reply_that_is_no_no_note_does_not_reopen_it_and_says_so() {
    let (rig, runner) = ruling("keeps", "feat: add it", Merging::Auto, PANEL);
    rig.forge.threads.grant_write("octocat");
    script(&rig, ["A", "A", "A", "A"]);
    voted(&rig, &runner);
    rig.clock.advance(5);

    rig.writes_on(71, "octocat", "1 yes");
    rig.next_look();
    step(&runner).unwrap();

    let said = super::comments(&rig).pop().expect("kelpie replied");
    assert!(said.contains("Ruling 1 was not reopened"), "{said}");
    assert!(said.contains("only a `no <note>` reopens it"), "{said}");
}

#[test]
fn a_decision_whose_work_item_has_finished_cannot_be_reopened() {
    let (rig, runner) = ruling("finished", "feat: add it", Merging::Auto, PANEL);
    rig.forge.threads.grant_write("octocat");
    script(&rig, ["A", "A", "A", "A"]);
    voted(&rig, &runner);
    // The yes merges it.
    step(&runner).unwrap();
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap();
    rig.clock.advance(5);

    rig.writes_on(71, "octocat", "1 no too late");
    rig.next_look();
    step(&runner).unwrap();

    let said = super::comments(&rig).pop().expect("kelpie replied");
    assert!(said.contains("Ruling 1 was not reopened"), "{said}");
    assert!(said.contains("finished"), "{said}");
}
