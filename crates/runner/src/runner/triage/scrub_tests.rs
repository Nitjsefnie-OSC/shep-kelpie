//! Triage posts checked through the rig's research, wording and forge ports

use std::path::Path;
use std::sync::Arc;

use super::ask_tests::{
    A_WORDS, ASK_WORDS, asked, asked_second, filed, filed_before, settled, wrote,
};
use super::pick_tests::{MAINTAINER, issue, list};
use super::research_tests::{calls, facts, reply};
use crate::local_paths::LocalPaths;
use crate::ports::{GuardedVoice, Role};
use crate::runner::step;
use crate::test::Scripted;

#[test]
fn a_question_naming_the_me_placeholder_is_posted_as_written() {
    let question = "Should `/home/me` be allowed?";
    let (rig, runner) = filed(
        "pick = \"maintainer\"",
        MAINTAINER,
        asked_second(question, "Yes"),
    );

    assert_eq!(step(&runner).unwrap(), None);

    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.contains(question));
    assert!(comments[0].1.starts_with(ASK_WORDS));
    assert_eq!(calls(&rig, Role::Representative).len(), 1);
}

#[test]
fn questions_naming_a_home_folder_are_left_out_before_wording_and_posting() {
    let path = ["/home/", "rin", "/notes"].concat();
    let mut found = asked_second(&format!("Should {path} be read?"), "Yes");
    found["open"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "question": "Where should the notes go?", "recommended": path,
        }));
    let (rig, runner) = filed("pick = \"maintainer\"", MAINTAINER, found);

    assert_eq!(step(&runner).unwrap(), None);

    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1, "the first attempt posts");
    assert!(comments[0].1.contains("❓ Q1 - Which one, 2?"));
    assert!(!comments[0].1.contains(&path));
    let facts = facts(&rig);
    assert!(!facts.contains(&path));
    assert!(!facts.contains("Where should the notes go?"));
    let notes = runner.lock().unwrap().take_notes();
    assert!(notes.iter().any(|n| n == "triage #30: 2 questions left out, naming a path on this machine, which names its user"), "{notes:?}");
    assert!(notes.iter().all(|n| !n.contains(&path)));
}

#[test]
fn a_title_naming_a_home_folder_is_left_out_of_the_terminal_prompt() {
    let path = ["/home/", "rin", "/notes"].concat();
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        let mut crash = issue(rig, 30, MAINTAINER, 5);
        crash.title = format!("Crash in {path}");
        crash.body = "It crashes when I run it.".to_owned();
        list(rig, crash);
    });

    assert_eq!(step(&runner).unwrap(), None);

    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(!comments[0].1.contains(&path));
    assert!(
        comments[0]
            .1
            .contains("/grill-with-docs Issue shep-pm/shep#30")
    );
    assert!(!comments[0].1.contains("Crash in"));
    assert_eq!(calls(&rig, Role::Representative).len(), 1);
    let notes = runner.lock().unwrap().take_notes();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("1 titles left out, naming a path on this machine"))
    );
    assert!(notes.iter().all(|n| !n.contains(&path)));
}

#[test]
fn wording_naming_a_home_folder_is_worded_once_more_with_the_reason() {
    let path = ["/home/", "rin", "/notes"].concat();
    let words = Box::leak(format!("Read {path} before answering.").into_boxed_str());
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        rig.claude
            .script_for(Role::Representative, [Scripted::Say(words)]);
    });

    assert_eq!(step(&runner).unwrap(), None);

    let calls = calls(&rig, Role::Representative);
    assert_eq!(calls.len(), 2);
    assert!(calls[1].call.prompt.contains(
        "The last wording named a path on this machine, which names its user. Leave it out."
    ));
    assert!(!calls[1].call.prompt.contains(&path));
    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.starts_with(ASK_WORDS));
    assert!(!comments[0].1.contains(&path));
}

#[test]
fn clean_wording_leaves_no_refusal_reason_for_the_authors_answers() {
    let path = ["/home/", "rin", "/notes"].concat();
    let words = Box::leak(format!("Read {path} before answering.").into_boxed_str());
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        rig.claude
            .script_for(Role::Representative, [Scripted::Say(words)]);
    });
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.github.comments().len(), 1);
    let worded = calls(&rig, Role::Representative);
    assert_eq!(worded.len(), 2);
    assert!(worded[1].call.prompt.contains("The last wording named"));

    wrote(&rig, MAINTAINER, "Q1: Linux, on start.");
    rig.claude.script_for(Role::Triager, [reply(settled())]);
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(A_WORDS)]);
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);

    let worded = calls(&rig, Role::Representative);
    assert_eq!(worded.len(), 3);
    assert!(!worded[2].call.prompt.contains("The last wording named"));
    assert_eq!(rig.github.comments().len(), 2);
}

#[test]
fn a_forge_refusal_is_explained_and_the_same_body_is_not_posted_again() {
    const WORDS: &str = "Read /forge-only/notes before answering.";
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        rig.claude.script_for(
            Role::Representative,
            [Scripted::Say(WORDS), Scripted::Say(WORDS)],
        );
    });
    // The forge knows a folder the runner's own check was not given.
    let voice = {
        let mut held = runner.lock().unwrap();
        let voice = Arc::clone(&held.ports.voice);
        held.ports.voice = Arc::new(GuardedVoice::new(
            Arc::clone(&voice),
            LocalPaths::new([Path::new("/forge-only")]),
        ));
        voice
    };
    assert_eq!(step(&runner).unwrap(), None);
    assert!(rig.github.comments().is_empty());

    // This forge now accepts anything: a repeated body must be stopped by triage.
    runner.lock().unwrap().ports.voice = voice;
    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert!(
        rig.github.comments().is_empty(),
        "the refused body was not sent twice"
    );
    let worded = calls(&rig, Role::Representative);
    assert_eq!(worded.len(), 2);
    assert!(worded[1].call.prompt.contains(
        "The last wording named a path on this machine, which names its user. Leave it out."
    ));
    assert!(!worded[1].call.prompt.contains("/forge-only"));

    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Triager).len(), 1, "research is kept");
    assert_eq!(calls(&rig, Role::Representative).len(), 3);
    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.starts_with(ASK_WORDS));
}

#[test]
fn an_earlier_refused_body_is_not_posted_after_a_different_refusal() {
    const A: &str = "Read /forge-only/first before answering.";
    const B: &str = "Read /forge-only/second before answering.";
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        rig.claude.script_for(
            Role::Representative,
            [Scripted::Say(A), Scripted::Say(B), Scripted::Say(A)],
        );
    });
    let voice = {
        let mut held = runner.lock().unwrap();
        let voice = Arc::clone(&held.ports.voice);
        held.ports.voice = Arc::new(GuardedVoice::new(
            Arc::clone(&voice),
            LocalPaths::new([Path::new("/forge-only")]),
        ));
        voice
    };
    for _ in 0..2 {
        assert_eq!(step(&runner).unwrap(), None);
        assert!(rig.github.comments().is_empty());
        rig.next_look();
    }

    runner.lock().unwrap().ports.voice = voice;
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Representative).len(), 3);
    assert!(
        rig.github.comments().is_empty(),
        "the earlier refused body was not sent twice"
    );

    rig.next_look();
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Representative).len(), 4);
    assert_eq!(calls(&rig, Role::Triager).len(), 1);
    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.starts_with(ASK_WORDS));
}

#[test]
fn two_refused_wordings_count_as_a_failure_and_five_failures_stop_the_issue() {
    let path = ["/home/", "rin", "/notes"].concat();
    let words = Box::leak(format!("Read {path} before answering.").into_boxed_str());
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, asked(), |rig| {
        rig.claude
            .script_for(Role::Representative, (0..10).map(|_| Scripted::Say(words)));
    });

    for attempt in 1..=5 {
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(calls(&rig, Role::Representative).len(), attempt * 2);
        assert!(rig.github.comments().is_empty());
        rig.next_look();
    }
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(calls(&rig, Role::Representative).len(), 10);
    let notes = runner.lock().unwrap().take_notes();
    assert!(notes.iter().any(|n| n.contains("it failed 5 times")));
    assert!(notes.iter().all(|n| !n.contains(&path)));
}

#[test]
fn files_tests_and_criteria_naming_this_machine_are_left_out() {
    let file = "%2Fhome%2Frin%2Fnotes";
    let path = ["/home/", "rin", "/notes"].concat();
    let mut found = asked();
    found["files"] = serde_json::json!([file, "README.md"]);
    found["tests"] = serde_json::json!([format!("Run {path}"), "runner::triage"]);
    found["criteria"] = serde_json::json!([format!("Read {path}"), "The comment posts"]);
    let (rig, runner) = filed_before("pick = \"maintainer\"", MAINTAINER, found, |rig| {
        rig.land(file, "notes");
    });

    assert_eq!(step(&runner).unwrap(), None);

    let comments = rig.github.comments();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].1.contains("README.md"));
    assert!(!comments[0].1.contains(file));
    let facts = facts(&rig);
    assert!(facts.contains("runner::triage"));
    assert!(facts.contains("The comment posts"));
    assert!(!facts.contains(&path));
    assert!(!facts.contains(file));
    let notes = runner.lock().unwrap().take_notes();
    for part in ["files", "tests", "criteria"] {
        assert!(
            notes
                .iter()
                .any(|n| n.contains(&format!("1 {part} left out, naming a path on this machine"))),
            "{notes:?}"
        );
    }
    assert!(
        notes
            .iter()
            .all(|n| !n.contains(&path) && !n.contains(file))
    );
}
