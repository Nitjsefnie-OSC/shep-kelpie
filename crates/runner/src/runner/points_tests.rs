//! The project's `turn_start`, `turn_end`, `reset`, `ruling`, `ci` and `done`
//! hooks, through the runner's stand-ins: the points kelpie's own work item
//! loop runs, outside any sandbox

use std::sync::Mutex;

use serde_json::Value;

use crate::ports::{CLEAN, Checks};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

const WORD: &str = "Acme";

// What the worker and the ruling read of a hook's refusal.
const TURN_END_REFUSED: &str = "the `turn_end` hook `forbid-word.sh` refused: it names a word \
                                the project keeps private";

fn phase(rig: &Rig, runner: &Mutex<Runner>) -> String {
    let status = rig.ask(runner, "status", None);
    status["work_item"]["phase"]["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn prompts(rig: &Rig) -> Vec<String> {
    (rig.claude.seen().into_iter())
        .map(|s| s.call.prompt)
        .collect()
}

fn notes(runner: &Mutex<Runner>) -> String {
    runner.lock().unwrap().take_notes().join("\n")
}

// The payloads a recording hook was run with, one JSON object a line.
fn payloads(rig: &Rig, name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(rig.home.path().join(name)).unwrap_or_default();
    (text.lines())
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

// A hook at `points` that appends each payload it is given to `name`'s record.
fn record(rig: &Rig, points: &[&str], name: &str) {
    let file = rig.home.path().join(name);
    let script = format!("cat >> {0}; echo >> {0}", file.display());
    rig.hook(&format!("{name}.sh"), points, &script);
}

// A running project whose `turn_end` hook refuses `WORD`, with issue 7 in
// flight and its pull request, 71, open.
fn ending() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.forbid_word(&["turn_end"], WORD);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    (rig, runner)
}

#[test]
fn a_turn_the_hook_refuses_goes_back_to_the_worker_with_the_reason_once() {
    let (rig, runner) = ending();
    rig.claude.script([
        Scripted::PushSay("work.txt", "one\n", "built for Acme"),
        Scripted::PushSay("work.txt", "two\n", "built for a customer"),
    ]);

    step(&runner).unwrap(); // the turn's last message is refused
    assert_eq!(phase(&rig, &runner), "implement", "no review reads it");
    step(&runner).unwrap(); // sent back with the reason, and answers again

    let told = prompts(&rig).pop().unwrap();
    assert!(told.contains(TURN_END_REFUSED), "{told}");
    assert!(!told.contains(WORD), "{told}");
    assert_eq!(phase(&rig, &runner), "review", "the second turn passes");
}

#[test]
fn a_turn_the_hook_refuses_twice_parks_on_a_ruling_naming_the_hook() {
    let (rig, runner) = ending();
    rig.claude.script([
        Scripted::PushSay("work.txt", "one\n", "built for Acme"),
        Scripted::PushSay("work.txt", "two\n", "still for Acme"),
    ]);

    step(&runner).unwrap(); // refused, and sent back
    let Some(StepReport::Failed { question, .. }) = step(&runner).unwrap() else {
        panic!("a turn refused twice did not park");
    };

    assert_eq!(phase(&rig, &runner), "ruling");
    assert!(question.contains(TURN_END_REFUSED), "{question}");
    assert!(question.contains("refused the turn twice"), "{question}");
}

// A project parked on its merge ruling, 1, with `points` recording what they are told.
fn parked_recording(points: &[&str]) -> (Rig, Mutex<Runner>, String) {
    Rig::parked_set("koji", |rig| record(rig, points, "told"))
}

#[test]
fn a_ruling_is_told_to_the_ruling_hook_with_its_kind_question_and_choices() {
    let (rig, runner, _) = parked_recording(&["ruling"]);

    let told = payloads(&rig, "told");

    let [ruling] = &told[..] else {
        panic!("one ruling was raised: {told:?}");
    };
    assert_eq!(ruling["point"], "ruling");
    assert_eq!(ruling["role"], "kelpie");
    assert_eq!(ruling["id"], "71");
    assert_eq!(ruling["issue"], "7");
    assert_eq!(ruling["pull_request"], 71);
    assert_eq!(ruling["kind"], "merge");
    assert_eq!(
        ruling["choices"],
        serde_json::json!(["yes", "no", "rework"])
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(ruling["question"], status["rulings"][0]["question"]);
    // Told once, however many passes the ruling waits.
    step(&runner).unwrap();
    step(&runner).unwrap();
    assert_eq!(payloads(&rig, "told").len(), 1);
}

#[test]
fn a_question_ruling_takes_free_text_and_has_no_choices() {
    let rig = Rig::new("shep");
    record(&rig, &["ruling"], "told");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(
        "<kelpie-question>\nWhich flag?\n</kelpie-question>",
    )]);

    step(&runner).unwrap();

    let told = payloads(&rig, "told");
    let [ruling] = &told[..] else {
        panic!("one ruling was raised: {told:?}");
    };
    assert_eq!(ruling["kind"], "question");
    assert_eq!(ruling["choices"], serde_json::json!([]));
    assert!(ruling["question"].as_str().unwrap().contains("Which flag?"));
}

#[test]
fn a_second_ruling_on_the_same_work_item_is_told_too() {
    let rig = Rig::new("shep");
    record(&rig, &["ruling"], "told");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    for _ in 0..2 {
        rig.claude.script([Scripted::Say(
            "<kelpie-question>\nWhich flag?\n</kelpie-question>",
        )]);
        step(&runner).unwrap();
        rig.ask(&runner, "rule", Some("7 answer use --dry-run"));
        rig.clock.advance(60);
    }
    runner.lock().unwrap().settle_notices();

    let told = payloads(&rig, "told");

    let ids: Vec<Value> = told.iter().map(|t| t["id"].clone()).collect();
    assert_eq!(
        ids,
        [serde_json::json!("7"), serde_json::json!("7")],
        "{told:?}"
    );
}

#[test]
fn a_ruling_a_restart_finds_is_not_told_again() {
    let (rig, runner, _) = parked_recording(&["ruling"]);
    drop(runner);

    let runner = rig.open().unwrap();
    step(&runner).unwrap();

    assert_eq!(payloads(&rig, "told").len(), 1);
}

#[test]
fn a_notice_that_exits_one_is_logged_with_its_name_and_the_loop_goes_on() {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        rig.hook(
            "fails.sh",
            &["ruling"],
            "echo 'no webhook for you' >&2; exit 1",
        );
    });

    let logged = notes(&runner);

    assert!(logged.contains("the `ruling` hook `fails.sh`"), "{logged}");
    assert!(logged.contains("no webhook for you"), "{logged}");
    // The ruling still stands, and can be answered.
    assert_eq!(phase(&rig, &runner), "ruling");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(phase(&rig, &runner), "merge");
}

#[test]
fn a_notice_that_hangs_is_stopped_logged_and_holds_nothing_up() {
    let rig = Rig::new("koji");
    rig.hook("hangs.sh", &["ci"], "sleep 30");
    let runner = rig.open().unwrap();
    let timeout = std::time::Duration::from_millis(200);
    {
        let runner = runner.lock().unwrap();
        let hooks = runner.hooks.get().as_ref().clone();
        runner.hooks.replace(hooks.with_timeout(timeout));
    }
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.ci_on_now(&runner);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // the turn pushes, and the review starts
    let head = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge.set_checks(&head, Checks::Passed);

    let started = std::time::Instant::now();
    for _ in 0..8 {
        rig.verdict(&runner);
    }

    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    let logged = notes(&runner);
    assert!(logged.contains("the `ci` hook `hangs.sh`"), "{logged}");
    assert!(logged.contains("ran past"), "{logged}");
}

#[test]
fn a_slow_notice_does_not_keep_the_runner_waiting_and_notices_run_in_order() {
    use crate::hooks::Role;
    use crate::settings::HookPoint;
    let rig = Rig::new("koji");
    let file = rig.home.path().join("told");
    // The first notice sleeps before it records, and the second records at once.
    let script = format!(
        "read line; case \"$line\" in *'\"n\":1'*) sleep 1;; esac; echo \"$line\" >> {}",
        file.display()
    );
    rig.hook("slow.sh", &["done"], &script);
    let runner = rig.open().unwrap();

    let started = std::time::Instant::now();
    {
        let runner = runner.lock().unwrap();
        for n in 1..=2 {
            runner.tell_hooks(HookPoint::Done, Role::Kelpie, serde_json::json!({ "n": n }));
        }
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "the runner waited for a hook"
        );
        runner.settle_notices();
    }

    let told: Vec<u64> = payloads(&rig, "told")
        .iter()
        .map(|p| p["n"].as_u64().unwrap())
        .collect();
    assert_eq!(told, [1, 2], "told in the order they came");
}

#[test]
fn a_notice_is_running_from_the_time_it_is_told_until_its_hook_ends() {
    use crate::hooks::Role;
    use crate::settings::HookPoint;
    let rig = Rig::new("koji");
    let file = rig.home.path().join("told");
    let script = format!("read line; sleep 1; echo \"$line\" >> {}", file.display());
    rig.hook("slow.sh", &["done"], &script);
    let runner = rig.open().unwrap();
    let runner = runner.lock().unwrap();
    assert!(!runner.notices_running(), "nothing told yet");

    runner.tell_hooks(HookPoint::Done, Role::Kelpie, serde_json::json!({ "n": 1 }));
    assert!(runner.notices_running(), "told and not yet ended");

    runner.settle_notices();
    assert!(!runner.notices_running(), "ended");
}

#[test]
fn the_ci_hook_is_told_a_head_that_settled_green_with_no_failed_checks() {
    let (rig, runner, head) = parked_recording(&["ci"]);

    let told = payloads(&rig, "told");

    let [ci] = &told[..] else {
        panic!("CI settled once: {told:?}");
    };
    assert_eq!(ci["point"], "ci");
    assert_eq!(ci["role"], "kelpie");
    assert_eq!(ci["issue"], "7");
    assert_eq!(ci["pull_request"], 71);
    assert_eq!(ci["head"], head.as_str());
    assert_eq!(ci["conclusion"], "passed");
    assert_eq!(ci["failed"], serde_json::json!([]));
    step(&runner).unwrap();
    assert_eq!(payloads(&rig, "told").len(), 1, "told once for the head");
}

#[test]
fn the_ci_hook_is_told_the_names_of_the_checks_that_failed() {
    let rig = Rig::new("koji");
    record(&rig, &["ci"], "told");
    let runner = rig.open().unwrap();
    rig.ci_on_now(&runner);
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();
    let head = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["build".into(), "lint".into()]));

    rig.verdict(&runner);

    let told = payloads(&rig, "told");
    let [ci] = &told[..] else {
        panic!("CI settled once: {told:?}");
    };
    assert_eq!(ci["conclusion"], "failed");
    assert_eq!(ci["failed"], serde_json::json!(["build", "lint"]));
    assert_eq!(ci["head"], head.as_str());
}

#[test]
fn the_done_hook_is_told_a_work_item_that_merged_with_its_history_entry() {
    let (rig, runner, head) = parked_recording(&["done"]);
    rig.ask(&runner, "rule", Some("71 yes"));

    step(&runner).unwrap(); // marks the draft ready
    rig.clock.advance(crate::runner::CHECKS_SETTLE);
    step(&runner).unwrap(); // merges, and the work item ends

    let told = payloads(&rig, "told");
    let [done] = &told[..] else {
        panic!("one work item ended: {told:?}");
    };
    assert_eq!(done["point"], "done");
    assert_eq!(done["issue"], "7");
    assert_eq!(done["pull_request"], 71);
    assert_eq!(done["merged"], true);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(done["history"], status["history"][0]);
    assert_eq!(done["history"]["issue"], "7");
    let _ = head;
}

#[test]
fn the_done_hook_is_told_a_work_item_that_ended_unmerged_too() {
    let rig = Rig::new("shep");
    record(&rig, &["done"], "told");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));

    rig.ask(&runner, "drop", None);
    step(&runner).unwrap();

    let told = payloads(&rig, "told");
    let [done] = &told[..] else {
        panic!("one work item ended: {told:?}");
    };
    assert_eq!(done["issue"], "7");
    assert_eq!(done["merged"], false);
}

#[test]
fn the_turn_start_hook_is_told_each_prompt_kelpie_sends_a_worker_and_a_reviewer() {
    let (rig, _runner, _) = parked_recording(&["turn_start"]);

    let told = payloads(&rig, "told");

    let [worker, reviewer] = &told[..] else {
        panic!("a worker's turn and a reviewer's round: {told:?}");
    };
    assert_eq!(worker["point"], "turn_start");
    assert_eq!(worker["role"], "worker");
    assert_eq!(worker["recipient"], "worker");
    assert_eq!(worker["issue"], 7);
    assert_eq!(worker["phase"], "implement");
    assert_eq!(worker["session"], "new");
    assert!(
        worker["prompt"].as_str().unwrap().contains("issue #7"),
        "{worker}"
    );
    assert_eq!(reviewer["role"], "kelpie");
    assert_eq!(reviewer["recipient"], "reviewer");
    assert_eq!(reviewer["issue"], 7);
    assert_eq!(reviewer["phase"], "review");
    assert_eq!(reviewer["session"], "new");
}

#[test]
fn a_later_turn_is_a_resumed_session_and_its_prompt_is_the_one_sent() {
    let rig = Rig::new("shep");
    rig.forbid_word(&["turn_end"], WORD);
    record(&rig, &["turn_start"], "told");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::PushSay("work.txt", "one\n", "built for Acme"),
        Scripted::PushSay("work.txt", "two\n", "built for a customer"),
    ]);

    step(&runner).unwrap(); // refused by turn_end, sent back
    step(&runner).unwrap();

    let told = payloads(&rig, "told");
    let [first, second] = &told[..] else {
        panic!("two turns: {told:?}");
    };
    assert_eq!(first["session"], "new");
    assert_eq!(second["session"], "resumed");
    assert_eq!(second["prompt"], prompts(&rig)[1].as_str());
    assert!(second["prompt"].as_str().unwrap().contains("turn_end"));
}

#[test]
fn a_prompt_the_turn_start_hook_refuses_is_not_sent_and_parks_on_a_ruling_naming_the_hook() {
    let rig = Rig::new("shep");
    rig.hook("gate.sh", &["turn_start"], "echo 'not now' >&2; exit 2");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));

    let Some(StepReport::Failed { question, .. }) = step(&runner).unwrap() else {
        panic!("a refused prompt did not park");
    };

    assert!(
        question.contains("the `turn_start` hook `gate.sh` refused: not now"),
        "{question}"
    );
    assert_eq!(phase(&rig, &runner), "ruling");
    assert!(prompts(&rig).is_empty(), "the worker was sent nothing");
}

#[test]
fn a_reviewer_s_prompt_the_turn_start_hook_refuses_is_not_sent() {
    let rig = Rig::new("shep");
    rig.hook(
        "gate.sh",
        &["turn_start"],
        "grep -q '\"recipient\":\"reviewer\"' && { echo 'no reviewers today' >&2; exit 2; }; exit 0",
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);

    step(&runner).unwrap(); // the worker's turn
    step(&runner).unwrap(); // review round 1, qwen: clean by default
    let before = prompts(&rig).len();
    let report = step(&runner).unwrap(); // review round 2, claude: refused

    assert_eq!(prompts(&rig).len(), before, "the reviewer was sent nothing");
    // A prompt no hook lets through is a call that failed: the round is tried again,
    // and a reviewer that keeps failing is skipped, as for any failed call.
    let Some(StepReport::GateFailed { reason, .. }) = report else {
        panic!("a refused reviewer did not fail its call: {report:?}");
    };
    assert!(
        reason.contains("the `turn_start` hook `gate.sh` refused: no reviewers today"),
        "{reason}"
    );
    assert_eq!(phase(&rig, &runner), "review");
}

#[test]
fn a_reviewer_skipped_after_the_turn_start_hook_refused_it_names_the_hook_in_the_skip() {
    let rig = Rig::new("shep");
    rig.hook(
        "gate.sh",
        &["turn_start"],
        "grep -q '\"recipient\":\"reviewer\"' && { echo 'no reviewers today' >&2; exit 2; }; exit 0",
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);

    step(&runner).unwrap(); // the worker's turn
    step(&runner).unwrap(); // review round 1, qwen: clean by default
    step(&runner).unwrap(); // round 2, claude: refused, tried again
    step(&runner).unwrap(); // refused again
    let report = step(&runner).unwrap(); // refused a third time: skipped

    let Some(StepReport::ReviewerSkipped { reason, .. }) = report else {
        panic!("a reviewer the hook keeps refusing was not skipped: {report:?}");
    };
    assert!(
        reason.contains("the `turn_start` hook `gate.sh` refused: no reviewers today"),
        "{reason}"
    );
}

// Issue 7's build turn opened pull request 71 with its session at 200k tokens of
// context, and round 1 found one thing, so a fix turn is next, behind a `/compact`.
fn compact_due(rig: &Rig) -> Mutex<Runner> {
    use crate::ports::{Finding, Severity};
    use crate::test::ScriptedRound;
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::PushAt("work.txt", "work\n", 200_000)]);
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
    runner
}

#[test]
fn the_reset_hook_is_told_before_a_worker_is_compacted_and_a_failure_is_logged() {
    let rig = Rig::new("shep");
    record(&rig, &["reset"], "told");
    rig.hook("fails.sh", &["reset"], "exit 1");
    let runner = compact_due(&rig);
    rig.claude.script([
        Scripted::Say("compacted"),
        Scripted::Push("fixed.txt", "fixed\n"),
    ]);

    step(&runner).unwrap(); // the `/compact`
    step(&runner).unwrap(); // the fix turn, behind it

    let told = payloads(&rig, "told");
    let [reset] = &told[..] else {
        panic!("one reset: {told:?}");
    };
    assert_eq!(reset["point"], "reset");
    assert_eq!(reset["role"], "worker");
    assert_eq!(reset["issue"], 7);
    assert_eq!(reset["recipient"], "worker");
    assert_eq!(reset["kind"], "compact");
    assert_eq!(reset["handoff"], serde_json::Value::Null);
    let logged = notes(&runner);
    assert!(logged.contains("the `reset` hook `fails.sh`"), "{logged}");
    assert_eq!(
        rig.claude.all_seen().len(),
        3,
        "the compaction and fix both ran"
    );
}

#[test]
fn the_turn_end_hook_runs_every_turn_with_the_issue_phase_message_and_head() {
    let rig = Rig::new("shep");
    record(&rig, &["turn_end"], "ends");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    // No pull request is open, so no handover is due: the hook runs all the same.
    rig.claude
        .script([Scripted::PushSay("work.txt", "one\n", "pushed it")]);

    step(&runner).unwrap();

    let [ended] = &payloads(&rig, "ends")[..] else {
        panic!("one turn ended");
    };
    assert_eq!(ended["point"], "turn_end");
    assert_eq!(ended["role"], "worker");
    assert_eq!(ended["issue"], "7");
    assert_eq!(ended["phase"], "implement");
    assert_eq!(ended["last_message"], "pushed it");
    assert_eq!(ended["head"].as_str().map(str::len), Some(40));
}
