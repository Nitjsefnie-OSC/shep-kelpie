//! A project's `handover` hooks, through the runner's stand-ins: a branch a
//! hook refuses goes back to the worker before any review reads it

use std::sync::Mutex;

use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

const WORD: &str = "Acme";

// The hook's reason, as the worker and the ruling read it.
const REFUSED: &str = "the `handover` hook `forbid-word.sh` refused: it names a word the project \
                       keeps private";

// A running project whose handover hook refuses `WORD`, with issue 7 in
// flight and its pull request, 71, open.
fn handing_over() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.forbid_word(&["handover"], WORD);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    (rig, runner)
}

fn phase(rig: &Rig, runner: &Mutex<Runner>) -> String {
    let status = rig.ask(runner, "status", None);
    status["work_item"]["phase"]["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_branch_the_hook_refuses_goes_back_to_the_worker_and_is_reviewed_once_it_passes() {
    // The hook reads the worktree it runs in, since a fix on top leaves the
    // refused commit in the range, where a diff would still show it.
    let rig = Rig::new("shep");
    rig.hook(
        "forbid-word.sh",
        &["handover"],
        "if grep -q Acme work.txt; then echo 'it names a word the project keeps private' >&2; exit 2; fi",
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::Push("work.txt", "built for Acme\n"),
        Scripted::Push("work.txt", "built for a customer\n"),
    ]);

    step(&runner).unwrap(); // the turn pushes a branch the hook refuses
    assert_eq!(phase(&rig, &runner), "implement", "no review reads it");
    step(&runner).unwrap(); // sent back with the hook's reason, and pushes again

    let prompts: Vec<String> = (rig.claude.seen().into_iter())
        .map(|s| s.call.prompt)
        .collect();
    let told = prompts.last().unwrap();
    assert!(told.contains(REFUSED), "{told}");
    assert!(!told.contains(WORD), "{told}");
    assert_eq!(phase(&rig, &runner), "review", "the second push passes");
}

#[test]
fn a_branch_the_hook_refuses_twice_parks_on_a_ruling_naming_the_hook() {
    let (rig, runner) = handing_over();
    rig.claude.script([
        Scripted::Push("work.txt", "built for Acme\n"),
        Scripted::Push("work.txt", "still for Acme\n"),
    ]);

    step(&runner).unwrap(); // refused, and sent back
    let Some(StepReport::Failed { question, .. }) = step(&runner).unwrap() else {
        panic!("a branch refused twice did not park");
    };

    assert_eq!(phase(&rig, &runner), "ruling");
    assert!(question.contains(REFUSED), "{question}");
    assert!(question.contains("refused the branch twice"), "{question}");
}

// The hook reads the worktree it runs in, as the test above does.
fn refuses_acme_in_work() -> Rig {
    let rig = Rig::new("shep");
    rig.hook(
        "forbid-word.sh",
        &["handover"],
        "if grep -q Acme work.txt; then echo 'it names a word the project keeps private' >&2; exit 2; fi",
    );
    rig
}

// A turn that ended before its pull request was seen leaves the item to the
// next step, which finds it. That is a handover too, so the hook judges the
// branch there before any review reads it.
#[test]
fn a_pull_request_found_after_the_turn_is_handed_over_before_review() {
    let rig = refuses_acme_in_work();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([
        Scripted::Push("work.txt", "built for Acme\n"),
        Scripted::Push("work.txt", "built for a customer\n"),
    ]);

    step(&runner).unwrap(); // pushes, and no pull request is open yet
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    step(&runner).unwrap(); // finds it, and the hook refuses the branch
    assert_eq!(phase(&rig, &runner), "implement", "no review reads it");

    step(&runner).unwrap(); // sent back with the hook's reason, and pushes again
    let prompts: Vec<String> = (rig.claude.seen().into_iter())
        .map(|s| s.call.prompt)
        .collect();
    let told = prompts.last().unwrap();
    assert!(told.contains(REFUSED), "{told}");
    assert_eq!(phase(&rig, &runner), "review", "the second push passes");
}

#[test]
fn a_pull_request_found_after_the_turn_that_the_hook_refuses_twice_parks() {
    let rig = refuses_acme_in_work();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([
        Scripted::Push("work.txt", "built for Acme\n"),
        Scripted::Push("work.txt", "still for Acme\n"),
    ]);

    step(&runner).unwrap(); // pushes, and no pull request is open yet
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    step(&runner).unwrap(); // found, and refused
    step(&runner).unwrap(); // sent back, and pushes again
    assert_eq!(phase(&rig, &runner), "ruling");
}

#[test]
fn a_hook_reads_the_commits_the_branch_holds_past_the_base() {
    let rig = Rig::new("shep");
    let seen = rig.home.path().join("seen.json");
    rig.hook(
        "record.sh",
        &["handover"],
        &format!("cat > {}", seen.display()),
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);

    step(&runner).unwrap();

    let payload: serde_json::Value = serde_json::from_slice(&std::fs::read(seen).unwrap()).unwrap();
    assert_eq!(payload["point"], "handover");
    assert_eq!(payload["role"], "worker");
    assert_eq!(payload["pull_request"], 71);
    assert_eq!(payload["branch"], "kelpie/7");
    let [commit] = payload["commits"].as_array().unwrap().as_slice() else {
        panic!("one commit is handed over: {payload}");
    };
    assert_eq!(commit["message"], "work.txt");
    assert!(payload["diff"].as_str().unwrap().contains("+work"));
    assert_eq!(phase(&rig, &runner), "review");
}
