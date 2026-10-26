//! A triager reply that is no triage, repaired once in the triager's own
//! session, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::json;

use super::pick_tests::{list, rig_with};
use super::research_tests::{WORDS, calls, crash, reply};
use crate::ports::{Role, Session};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

// #1287's reply: the entry in `open` lost the key in front of its recommendation
const DROPPED_KEY: Scripted =
    Scripted::Text(r#"{"type":null,"open":[{"question":"Which platform?","Linux"}]}"#);

// The same reply with the key back
fn mended() -> Scripted {
    let open = json!([{ "question": "Which platform?", "recommended": "Linux" }]);
    reply(json!({ "open": open }))
}

// The harnesses the triager runs on, as the rig's agent files name them
#[derive(Clone, Copy, Debug)]
enum On {
    ClaudeCode,
    Pi,
}

// A rig whose untouched issue 30 is researched on `on`
fn researching_on(on: On) -> (Rig, Mutex<Runner>) {
    let rig = match on {
        On::ClaudeCode => rig_with("pick = \"all\""),
        On::Pi => {
            let rig = rig_with("pick = \"all\"\nagent = \"pi-scout\"");
            rig.write_agent(
                "pi-scout",
                "---\nrole: triager\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                 url: http://box:11434/v1\ncontext: 65536\n---\nResearch it.\n",
            );
            rig
        }
    };
    let runner = rig.open().unwrap();
    list(&rig, crash(&rig, "It crashes when I run it."));
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(WORDS)]);
    (rig, runner)
}

#[test]
fn a_reply_with_a_dropped_key_is_repaired_in_the_same_pass_and_posts() {
    for on in [On::ClaudeCode, On::Pi] {
        let (rig, runner) = researching_on(on);
        rig.claude
            .script_for(Role::Triager, [DROPPED_KEY, mended()]);

        assert_eq!(step(&runner).unwrap(), None);

        let asked = calls(&rig, Role::Triager);
        assert_eq!(asked.len(), 2, "{on:?}: the repair is the only second call");
        let first = asked[0].call.session.id();
        assert_eq!(
            asked[1].call.session,
            Session::Resume(first.clone()),
            "{on:?}"
        );
        assert!(asked[1].call.prompt.contains("expected `:`"), "{on:?}");
        assert_eq!(rig.github.comments().len(), 1, "{on:?}: the comment posts");
    }
}

#[test]
fn a_repair_that_is_no_triage_either_is_tried_at_the_next_poll_and_counts_once() {
    for on in [On::ClaudeCode, On::Pi] {
        let (rig, runner) = researching_on(on);
        // Five tries of two calls each, then the reply that would post.
        let broken = std::iter::repeat_n(DROPPED_KEY, 10);
        rig.claude.script_for(Role::Triager, broken);
        rig.claude.script_for(Role::Triager, [mended()]);

        for tried in 1..=5 {
            assert_eq!(step(&runner).unwrap(), None);
            assert_eq!(calls(&rig, Role::Triager).len(), tried * 2, "{on:?}");
            assert_eq!(rig.github.comments(), [], "{on:?}: nothing posts");
            rig.next_look();
        }

        // The fifth try gave the issue up: a sixth poll calls nothing.
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(
            calls(&rig, Role::Triager).len(),
            10,
            "{on:?}: given up after five"
        );
    }
}

#[test]
fn a_reply_with_no_json_object_is_not_repaired() {
    let (rig, runner) = researching_on(On::ClaudeCode);
    rig.claude
        .script_for(Role::Triager, [Scripted::Text("I could not tell.")]);

    assert_eq!(step(&runner).unwrap(), None);

    assert_eq!(calls(&rig, Role::Triager).len(), 1);
}
