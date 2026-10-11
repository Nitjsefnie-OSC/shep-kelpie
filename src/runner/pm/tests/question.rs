//! A worker's question through the project manager: it answers, or passes
//! the question on with a recommendation

use std::sync::Mutex;

use serde_json::json;

use super::{PATIENCE, answered, step_until, with_pm};
use crate::ports::AgentError;
use crate::runner::flight::advance;
use crate::runner::{Pass, Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Hold, Rig, Scripted};

const CHOICES: &str = "Done so far.\n\n<kelpie-question>\nWhich flag?\n- `--dry-run`\n\
                       - `--check`\n</kelpie-question>\n";

const ASKS: &str = "I added the flag.\n\n<kelpie-question>\nShould it be `--dry-run` or \
                    `--check`?\n\nBoth appear in the docs.\n</kelpie-question>\n";

/// A project with a project manager, and issue 7's worker asking `question`
/// on its first turn, the ruling raised
fn asking(project: &str, question: &'static str) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = with_pm(project);
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(question)]);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Asked { id: 1, .. })
    ));
    (rig, runner)
}

#[test]
fn a_question_wakes_it_and_its_text_is_the_workers_next_turn() {
    let (rig, runner) = asking("rotom", ASKS);
    rig.claude.script([
        Scripted::Say(
            "{\"answer\": {\"item\": 7, \"text\": \"Use --dry-run: the issue says it matches \
             shep.\", \"why\": \"#7 names the flag\"}}",
        ),
        Scripted::Say("renamed"),
    ]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, acted, dropped) = answered(Some(report));
    assert_eq!(
        woke_for,
        [
            "#7's worker asks, on ruling 1:\n\n> Should it be `--dry-run` or `--check`?\n>\n> \
             Both appear in the docs."
        ]
    );
    assert_eq!(acted, ["answered ruling 1 on #7"]);
    assert_eq!(dropped, Vec::<String>::new());

    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"], json!([]));
    assert!(
        rig.alerts.posts().is_empty(),
        "the maintainer was never asked"
    );

    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ended { .. })
    ));
    let calls = rig.claude.calls();
    let resumed = &calls.last().unwrap().prompt;
    assert!(
        resumed.contains("PM answered: Use --dry-run: the issue says it matches shep."),
        "{resumed}"
    );
}

#[test]
fn the_worker_is_still_told_to_ask_for_what_only_the_maintainer_can_decide() {
    let told = crate::profile::INSTRUCTIONS;
    assert!(
        told.contains("a decision only the maintainer can make"),
        "{told}"
    );
}

#[test]
fn each_answer_and_its_why_is_in_the_notes_the_board_and_status() {
    let (rig, runner) = asking("pikachu", ASKS);
    let long = "x".repeat(2_500);
    let answer = format!(
        "{{\"answer\": {{\"item\": 7, \"text\": \"{long}\", \"why\": \"the issue says so\"}}}}"
    );
    rig.claude
        .script([Scripted::Say(Box::leak(answer.into_boxed_str()))]);
    step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));

    let line = "ruling 1 on #7: the project manager answered it: the issue says so";
    let notes = std::fs::read_to_string(rig.paths().pm.join("pm-notes.md")).unwrap();
    assert_eq!(notes, format!("- {line}\n"));
    let pm = &rig.ask(&runner, "status", None)["pm"];
    assert_eq!(
        pm["settled"],
        json!([{
            "ruling": 1,
            "issue": 7,
            "how": "answered",
            "why": "the issue says so",
        }])
    );
    step(&runner).unwrap();
    let board = std::fs::read_to_string(&rig.paths().board).unwrap();
    assert!(board.contains(line), "{board}");

    let calls = rig.claude.calls();
    let turn = &calls.last().unwrap().prompt;
    let sent = turn.split("PM answered: ").nth(1).unwrap().trim_end();
    assert_eq!(sent.chars().count(), 2_000 + " …".chars().count());
}

// The ruling's question as the maintainer is alerted of it, once the
// project manager has passed it on
fn passed_on(rig: &Rig, runner: &Mutex<Runner>, answer: &'static str) -> String {
    rig.claude.script([Scripted::Say(answer)]);
    let report = step_until(runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (_, acted, dropped) = answered(Some(report));
    assert_eq!(
        acted,
        ["put ruling 1 on #7 to the maintainer"],
        "{dropped:?}"
    );
    assert_eq!(step(runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let [(_, alert)] = rig.alerts.posts().try_into().unwrap();
    alert.text
}

#[test]
fn passing_a_question_on_puts_its_recommended_choice_first_in_the_list() {
    let (rig, runner) = asking("bulbasaur", CHOICES);
    let text = passed_on(
        &rig,
        &runner,
        "{\"answer\": {\"item\": 7, \"escalate\": \"--check\", \"why\": \"shep uses it\"}}",
    );
    assert_eq!(
        text,
        "The worker on issue #7 asks:\n\nWhich flag?\nPM recommends: `--check`\n\
         PM says: shep uses it\n- `--check`\n- `--dry-run`\n\n\
         `shep kelpie rule 1 <text>` sends the worker your answer."
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"][0]["question"], text);
    assert_eq!(status["rulings"][0]["alerted"], true);
    assert_eq!(
        rig.claude.calls().len(),
        1,
        "the worker waits on the maintainer"
    );
}

#[test]
fn a_recommendation_outside_the_list_is_added_as_its_first_line() {
    let (rig, runner) = asking("ivysaur", CHOICES);
    let text = passed_on(
        &rig,
        &runner,
        "{\"answer\": {\"item\": 7, \"escalate\": \"--plan\", \"why\": \"\"}}",
    );
    assert_eq!(
        text,
        "The worker on issue #7 asks:\n\nWhich flag?\nPM recommends: --plan\n\
         - --plan\n- `--dry-run`\n- `--check`\n\n\
         `shep kelpie rule 1 <text>` sends the worker your answer."
    );
}

#[test]
fn a_question_with_no_list_gets_the_recommendation_line_alone() {
    let (rig, runner) = asking("venusaur", ASKS);
    let text = passed_on(
        &rig,
        &runner,
        "{\"answer\": {\"item\": 7, \"escalate\": \"--check\", \"why\": \"\"}}",
    );
    assert!(
        text.contains("Both appear in the docs.\nPM recommends: --check\n\n"),
        "{text}"
    );
}

#[test]
fn an_answer_for_an_item_not_parked_on_a_question_is_dropped_and_the_ruling_stands() {
    let (rig, runner) = asking("charmander", ASKS);
    rig.claude.script([Scripted::Say(
        "{\"answer\": {\"item\": 9, \"text\": \"Use --check.\", \"why\": \"guess\"}}",
    )]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (_, acted, dropped) = answered(Some(report));
    assert_eq!(acted, Vec::<String>::new());
    assert_eq!(
        dropped,
        ["answer #9: not a work item parked on a question ruling"]
    );
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let [(_, alert)] = rig.alerts.posts().try_into().unwrap();
    assert_eq!(
        alert.text,
        "The worker on issue #7 asks:\n\nShould it be `--dry-run` or `--check`?\n\n\
         Both appear in the docs.\n\n\
         `shep kelpie rule 1 <text>` sends the worker your answer."
    );
}

#[test]
fn a_wake_that_fails_sends_the_question_to_the_maintainer_as_it_is() {
    let (rig, runner) = asking("squirtle", ASKS);
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude.script([Scripted::Fail(down)]);
    step_until(&runner, |r| matches!(r, StepReport::PmFailed { .. }));
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    assert_eq!(rig.alerts.posts().len(), 1);
}

#[test]
fn an_answer_after_the_maintainer_was_alerted_is_dropped() {
    let (rig, runner) = asking("raichu", ASKS);
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude.script([Scripted::Fail(down)]);
    step_until(&runner, |r| matches!(r, StepReport::PmFailed { .. }));
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));

    rig.clock.advance(11 * 60);
    rig.ask(&runner, "tell", Some("Use --check"));
    rig.claude.script([Scripted::Say(
        "{\"answer\": {\"item\": 7, \"text\": \"Use --check.\", \"why\": \"told\"}}",
    )]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (_, acted, dropped) = answered(Some(report));
    assert_eq!(acted, Vec::<String>::new());
    assert_eq!(
        dropped,
        ["answer #7: ruling 1 already went to the maintainer"]
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"][0]["id"], 1, "the ruling still waits");
    assert_eq!(rig.claude.calls().len(), 1, "the worker is not resumed");
}

#[test]
fn a_recommendation_naming_this_machine_is_withheld_from_the_post() {
    let (rig, runner) = asking("meowth", CHOICES);
    let home = rig.paths().pm.display().to_string();
    let answer = format!("{{\"answer\": {{\"item\": 7, \"escalate\": \"{home}\"}}}}");
    let text = passed_on(&rig, &runner, Box::leak(answer.into_boxed_str()));
    assert!(!text.contains(&home), "{text}");
    assert!(
        text.contains("PM recommends: (withheld: it names"),
        "{text}"
    );
}

#[test]
fn a_question_waits_at_most_one_wake_for_its_answer() {
    let (rig, runner) = asking("wartortle", ASKS);
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(PATIENCE), "the wake never began");
    assert!(rig.alerts.posts().is_empty(), "the maintainer waits for it");

    rig.clock.advance(10 * 60);
    step_until(&runner, |r| matches!(r, StepReport::Alerted { id: 1 }));
    hold.release();
}
