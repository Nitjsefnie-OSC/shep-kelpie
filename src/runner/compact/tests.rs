use std::sync::Mutex;

use serde_json::Value;

use crate::ports::{AgentError, CLEAN, Checks, Finding, Session, Severity};
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted, ScriptedRound};
use crate::usage::{FILE, read};

// Issue 7's build turn opened pull request 71 with its session's context at
// `context` tokens, and round 1 found one thing, so a fix turn is next.
fn fix_round_due(context: u64) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::PushAt("work.txt", "work\n", context)]);
    step(&runner).unwrap(); // the build turn opens the pull request
    rig.reviewer
        .script([ScriptedRound::Findings(vec![Finding::new(
            Severity::Medium,
            "src/lib.rs",
            3,
            "the flag is misnamed",
            "it reads as its opposite",
        )])]);
    step(&runner).unwrap(); // round 1's call
    step(&runner).unwrap(); // the findings go out
    (rig, runner)
}

fn ledger(rig: &Rig) -> Vec<Value> {
    let lines = read(&rig.paths().folder.join(FILE)).unwrap();
    lines
        .iter()
        .map(|line| serde_json::to_value(line).unwrap())
        .collect()
}

#[test]
fn a_session_past_150k_is_compacted_before_the_fix_round_s_prompt() {
    let (rig, runner) = fix_round_due(150_001);
    rig.claude.script([
        Scripted::Say("compacted"),
        Scripted::Push("fixed.txt", "fixed\n"),
    ]);
    let compacted = step(&runner).unwrap();
    assert!(
        matches!(
            compacted,
            Some(StepReport::WorkerCompacted { issue: 7, .. })
        ),
        "{compacted:?}"
    );
    let fixed = step(&runner).unwrap();
    assert!(matches!(fixed, Some(StepReport::Ended { .. })), "{fixed:?}");

    let [build, compact, fix] = rig.claude.all_seen().try_into().unwrap();
    assert_eq!(compact.call.prompt, "/compact");
    assert_eq!(
        compact.call.session,
        Session::Resume(build.call.session.id().clone())
    );
    assert_eq!(fix.call.session, compact.call.session);
    // What the turn needs is in its prompt, not only in the history compacted away.
    let prompt = &fix.call.prompt;
    assert!(
        prompt.starts_with("Kelpie compacted your session"),
        "{prompt}"
    );
    assert!(prompt.contains("issue #7"), "{prompt}");
    assert!(prompt.contains("pull request #71"), "{prompt}");
    assert!(
        prompt.contains("Round 1 of the review on your pull request #71 found 1"),
        "{prompt}"
    );
}

#[test]
fn a_session_at_150k_is_not_compacted() {
    let (rig, runner) = fix_round_due(150_000);
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap();

    let [_, fix] = rig.claude.all_seen().try_into().unwrap();
    assert!(
        fix.call.prompt.starts_with("Round 1 of the review"),
        "{}",
        fix.call.prompt
    );
}

#[test]
fn the_ledger_shows_the_compaction_as_its_own_call_with_the_issue_and_pull_request() {
    let (rig, runner) = fix_round_due(200_000);
    rig.claude.script([
        Scripted::Say("compacted"),
        Scripted::Push("fixed.txt", "fixed\n"),
    ]);
    step(&runner).unwrap();
    step(&runner).unwrap();

    let calls: Vec<_> = ledger(&rig)
        .into_iter()
        .filter(|line| line["role"] == "worker")
        .map(|line| {
            (
                line["kind"].clone(),
                line["issue"].clone(),
                line["pull_request"].clone(),
            )
        })
        .collect();
    let line = |kind: &str| (kind.into(), 7.into(), 71.into());
    assert_eq!(calls, [line("turn"), line("compact"), line("turn")]);
}

// A slash command only runs from the very start of a prompt, so the CI fix
// turn's skill must still lead it once the reminder is added.
#[test]
fn a_ci_fix_turn_after_a_compaction_still_starts_with_its_skill_s_command() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::PushAt("work.txt", "work\n", 200_000),
        Scripted::Text(CLEAN),
    ]);
    step(&runner).unwrap(); // the build turn opens the pull request
    step(&runner).unwrap(); // review round 1
    step(&runner).unwrap(); // review round 2
    let head = rig.forge.head_of("kelpie/7").unwrap();
    // The gate begins here, so CI blocks from now on.
    rig.ci_on_now(&runner);
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["lint".to_owned()]));
    rig.verdict(&runner); // CI is red, so the worker's next turn is a fix
    rig.claude.script([
        Scripted::Say("compacted"),
        Scripted::Push("fix.txt", "fixed\n"),
    ]);
    step(&runner).unwrap(); // the compaction
    step(&runner).unwrap(); // the fix

    let [_, compact, fix] = rig.claude.seen().try_into().unwrap();
    assert_eq!(compact.call.prompt, "/compact");
    let prompt = &fix.call.prompt;
    assert!(
        prompt.starts_with("/mattpocock:diagnosing-bugs Kelpie compacted your session"),
        "{prompt}"
    );
    assert!(
        prompt.contains("CI failed on your pull request #71"),
        "{prompt}"
    );
}

#[test]
fn a_compaction_that_fails_sends_the_turn_on_as_it_was() {
    let (rig, runner) = fix_round_due(200_000);
    let failed = AgentError::Failed(Harness::ClaudeCode, "no compaction".into());
    rig.claude.script([
        Scripted::Fail(failed),
        Scripted::Push("fixed.txt", "fixed\n"),
    ]);
    let report = step(&runner).unwrap();
    assert!(
        matches!(
            report,
            Some(StepReport::WorkerCompactFailed { issue: 7, .. })
        ),
        "{report:?}"
    );
    step(&runner).unwrap();

    let [_, _, fix] = rig.claude.all_seen().try_into().unwrap();
    assert!(
        fix.call.prompt.starts_with("Round 1 of the review"),
        "{}",
        fix.call.prompt
    );
}

// The settings file of a worker's first turn, under a `worker` table that
// sets `compact_window` to `window`, or leaves it at its default.
fn first_turn_settings(window: Option<&str>) -> Value {
    let rig = Rig::new("shep");
    if let Some(window) = window {
        rig.edit_settings(|s| {
            s.replace(
                "compact_window = 200000",
                &format!("compact_window = {window}"),
            )
        });
    }
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say("done")]);
    step(&runner).unwrap();
    let [turn] = rig.claude.all_seen().try_into().unwrap();
    turn.settings
}

#[test]
fn a_worker_s_settings_carry_the_compact_window_at_its_default_and_as_set() {
    assert_eq!(first_turn_settings(None)["autoCompactWindow"], 200_000);
    assert_eq!(
        first_turn_settings(Some("120000"))["autoCompactWindow"],
        120_000
    );
}

#[test]
fn a_compact_window_of_zero_leaves_it_out_of_a_worker_s_settings() {
    let settings = first_turn_settings(Some("0"));
    assert!(settings.get("autoCompactWindow").is_none());
    assert!(settings["hooks"].get("SessionStart").is_none());
}

#[test]
fn kelpie_s_own_compact_call_is_not_bounded_by_the_window() {
    let (rig, runner) = fix_round_due(150_001);
    rig.claude.script([
        Scripted::Say("compacted"),
        Scripted::Push("fixed.txt", "fixed\n"),
    ]);
    step(&runner).unwrap();
    step(&runner).unwrap();

    let [build, compact, fix] = rig.claude.all_seen().try_into().unwrap();
    assert_eq!(build.settings["autoCompactWindow"], 200_000);
    assert!(compact.settings.get("autoCompactWindow").is_none());
    assert_eq!(fix.settings["autoCompactWindow"], 200_000);
}
