//! The project's `turn_start`, `turn_end`, `reset`, `ruling`, `ci` and `done`
//! hooks, through the runner's stand-ins: the points kelpie's own work item
//! loop runs, outside any sandbox

use std::sync::Mutex;

use serde_json::Value;

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
    assert_eq!(ended["issue"], 7);
    assert_eq!(ended["phase"], "implement");
    assert_eq!(ended["last_message"], "pushed it");
    assert_eq!(ended["head"].as_str().map(str::len), Some(40));
}
