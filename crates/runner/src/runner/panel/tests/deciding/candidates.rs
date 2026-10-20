//! A worker's question that lists fewer than two options gets candidate
//! answers from the project manager, which the panel votes on

use std::sync::Mutex;

use super::{PANEL, SHADOW, asked, comments, leak, pending, script, voted};
use crate::ports::Role;
use crate::runner::{Runner, StepReport, step};
use crate::test::ticket;
use crate::test::{Rig, Scripted};

const NO_OPTIONS: &str = "Done so far.\n\n<kelpie-question>\nShould the flag be `--dry-run` or \
                          `--check`?\n\nBoth appear in the docs.\n</kelpie-question>\n";
const OPTIONS: &str = "Done so far.\n\n<kelpie-question>\nWhich flag?\n- `--dry-run`\n\
                       - `--check`\n</kelpie-question>\n";

/// The project manager's reply that passes ruling 71 on with `candidates`
fn drafts(candidates: &str) -> &'static str {
    leak(format!(
        "{{\"answer\": {{\"item\": 7, \"escalate\": true, \"candidates\": {candidates}, \
         \"why\": \"the docs name both\"}}}}"
    ))
}

/// A worker asking `question` on the turn that opens pull request 71, in a
/// project with a panel under `rulings`
fn asking(project: &str, question: &'static str, rulings: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.with_app();
    rig.maintainer("octocat");
    super::super::name_panel(&rig, true);
    let table = format!("\n[app.dogs.kelpie.rulings]\n{rulings}\n");
    rig.edit_settings(|s| format!("{s}{table}"));
    rig.forge.set_title(71, "feat: add it");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Say(question)]);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Asked { id, .. })
     if id == ticket(71)));
    (rig, runner)
}

/// The comment that asks the maintainer the question, if one was posted
fn question_comment(rig: &Rig) -> Option<String> {
    (comments(rig).into_iter()).find(|body| body.starts_with("### "))
}

#[test]
fn a_question_with_no_options_gets_candidates_and_a_panel_vote() {
    let (rig, runner) = asking("cand-none", NO_OPTIONS, SHADOW);
    let candidates = "[\"Use `--dry-run`\", \"Use `--check`\", \"Ask the maintainer\"]";
    // The project manager is woken for the question first, and then to vote.
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(drafts(candidates))]);
    script(&rig, ["B", "B", "B", "A"]);

    voted(&rig, &runner);

    let comment = question_comment(&rig).expect("the maintainer is asked");
    assert!(
        comment.starts_with("### Should the flag be `--dry-run` or `--check`?"),
        "{comment}"
    );
    assert!(comment.contains("Both appear in the docs."), "{comment}");
    for candidate in ["Use `--dry-run`", "Use `--check`", "Ask the maintainer"] {
        assert!(comment.contains(candidate), "{candidate}: {comment}");
    }
    assert!(
        comment.contains("**Recommendation: B** (3 of 4)"),
        "{comment}"
    );
    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
    let shown = (seen_votes(&rig, Role::Advisor)).join("\n");
    assert!(shown.contains("Use `--check`"), "{shown}");
}

/// What `role` was asked when it was asked to vote on ruling 71
fn seen_votes(rig: &Rig, role: Role) -> Vec<String> {
    (super::super::seen(rig, role).into_iter())
        .map(|s| s.call.prompt)
        .filter(|p| p.contains("vote on ruling 71"))
        .collect()
}

const THREE: &str = "[\"Use `--dry-run`\", \"Use `--check`\", \"Ask the maintainer\"]";

/// A question with no options, passed on with `THREE` and voted on by `votes`
/// under `rulings`
fn drafted(project: &str, rulings: &str, votes: [&str; 4]) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = asking(project, NO_OPTIONS, rulings);
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(drafts(THREE))]);
    script(&rig, votes);
    voted(&rig, &runner);
    (rig, runner)
}

#[test]
fn a_question_that_lists_options_is_voted_on_as_asked() {
    let (rig, runner) = asking("cand-listed", OPTIONS, SHADOW);
    // The project manager offers candidates, which a listed question drops.
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(drafts(THREE))]);
    script(&rig, ["A", "A", "B", "A"]);

    voted(&rig, &runner);

    let comment = question_comment(&rig).expect("the maintainer is asked");
    assert!(comment.contains("`--dry-run`"), "{comment}");
    assert!(comment.contains("`--check`"), "{comment}");
    for candidate in ["Use `--dry-run`", "Ask the maintainer"] {
        assert!(!comment.contains(candidate), "{candidate}: {comment}");
    }
    assert!(
        comment.contains("**Recommendation: A** (3 of 4)"),
        "{comment}"
    );
    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
}

#[test]
fn the_maintainer_s_free_text_answer_still_reaches_the_worker() {
    let (rig, runner) = drafted("cand-free", SHADOW, ["B", "B", "B", "A"]);
    let words = "Use --check, and say so in the help text.";
    rig.claude.script([Scripted::Say("renamed")]);

    rig.ask(&runner, "rule", Some(&format!("71 answer {words}")));
    step(&runner).unwrap();

    assert!(!pending(&rig), "the answer settled the ruling");
    let calls = rig.claude.calls();
    let resumed = &calls.last().expect("the worker was resumed").prompt;
    assert!(resumed.contains(words), "{resumed}");
}

#[test]
fn the_maintainer_may_answer_with_a_candidate_and_status_counts_the_question() {
    let (rig, runner) = drafted("cand-status", SHADOW, ["B", "B", "B", "A"]);

    rig.ask(&runner, "rule", Some("71 answer Use `--check`"));
    let panel = rig.ask(&runner, "status", None)["panel"].clone();

    assert_eq!(panel["decide"], "shadow");
    assert_eq!(panel["voted"], 1);
    assert_eq!(panel["answered"], 1);
    assert_eq!(panel["streak"], 1);
    assert_eq!(panel["pm_alone"]["question"], 1);
    assert_eq!(panel["experts"]["worker"]["agreed"], 1);
    assert_eq!(panel["experts"]["advisor"]["agreed"], 0);
}

#[test]
fn a_free_text_answer_counts_the_question_as_voted_on_and_answered() {
    let (rig, runner) = drafted("cand-text", SHADOW, ["B", "B", "B", "A"]);
    rig.claude.script([Scripted::Say("renamed")]);

    rig.ask(
        &runner,
        "rule",
        Some("71 answer Use --check and say so in the help"),
    );
    let panel = rig.ask(&runner, "status", None)["panel"].clone();

    assert_eq!(
        (panel["voted"].clone(), panel["answered"].clone()),
        (1.into(), 1.into())
    );
    assert_eq!(panel["streak"], 0, "no candidate was the answer");
    assert_eq!(panel["experts"]["worker"]["votes"], 1);
    assert_eq!(panel["experts"]["worker"]["agreed"], 0);
}

#[test]
fn a_panel_that_decides_on_a_candidate_sends_the_worker_that_answer() {
    let (rig, runner) = asking("cand-decides", NO_OPTIONS, PANEL);
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(drafts(THREE))]);
    script(&rig, ["B", "B", "B", "B"]);
    rig.claude.script([Scripted::Say("renamed")]);

    voted(&rig, &runner);
    step(&runner).unwrap();

    assert!(!pending(&rig), "the panel applied its decision");
    let calls = rig.claude.calls();
    let resumed = &calls.last().expect("the worker was resumed").prompt;
    assert!(resumed.contains("Use `--check`"), "{resumed}");
}

#[test]
fn a_panel_that_votes_to_ask_the_maintainer_leaves_the_question_to_them() {
    let (rig, runner) = drafted("cand-asks", PANEL, ["C", "C", "C", "C"]);

    assert!(pending(&rig), "nothing was sent to the worker");
    let comment = question_comment(&rig).expect("the maintainer is asked");
    assert!(comment.contains("Ask the maintainer"), "{comment}");
    assert!(
        comment.contains("the maintainer answers in their own words"),
        "{comment}"
    );
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("ruling 71: the panel's pick is to ask the maintainer"),
        "{notes}"
    );
}

const ONE_OPTION: &str = "Done so far.\n\n<kelpie-question>\nWhich flag?\n- `--dry-run`\n\
                          </kelpie-question>\n";

#[test]
fn a_full_list_still_offers_asking_the_maintainer_in_place_of_the_last_candidate() {
    let (rig, runner) = asking("cand-full", ONE_OPTION, SHADOW);
    rig.claude.script_for(
        Role::Pm,
        [Scripted::Say(drafts(
            "[\"Use A\", \"Use B\", \"Ask the maintainer\"]",
        ))],
    );
    script(&rig, ["A", "A", "B", "A"]);

    voted(&rig, &runner);

    let comment = question_comment(&rig).expect("the maintainer is asked");
    for kept in ["`--dry-run`", "Use A", "Ask the maintainer"] {
        assert!(comment.contains(kept), "{kept}: {comment}");
    }
    assert!(!comment.contains("Use B"), "{comment}");
}
