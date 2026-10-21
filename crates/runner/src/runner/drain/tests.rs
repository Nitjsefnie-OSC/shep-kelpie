//! Draining through the runner's stand-ins: what starts, what goes on, and
//! what `status` shows

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use crate::guard::Pause;
use crate::ports::{CLEAN, Cost, Role, Usage};
use crate::runner::gate::CHECKS_SETTLE;
use crate::runner::{Pass, Runner, advance, answer, step};
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

// A running project with issue 7 open and its first turn due
fn seven_due(project: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    (rig, runner)
}

fn draining(rig: &Rig, runner: &Mutex<Runner>) -> Value {
    rig.ask(runner, "status", None)["draining"].clone()
}

#[test]
fn a_drained_runner_starts_no_turn_until_it_is_undrained() {
    let (rig, runner) = seven_due("koji");
    assert_eq!(draining(&rig, &runner), json!(null), "not draining");
    assert_eq!(
        rig.ask(&runner, "drain", None)["draining"],
        json!({ "calls": [], "ceiling": 3600 })
    );
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.claude.calls(), [], "the due turn waits");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["turn"]["state"],
        "due"
    );

    assert_eq!(rig.ask(&runner, "undrain", None)["draining"], json!(null));
    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(&runner).unwrap();
    assert_eq!(rig.claude.calls().len(), 1, "the turn ran once undrained");
}

#[test]
fn drain_answers_the_calls_still_running_until_they_end() {
    let (rig, runner) = seven_due("golbat");
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the turn never began");
    assert_eq!(
        rig.ask(&runner, "drain", None)["draining"]["calls"],
        json!([{ "issue": "7", "role": "worker" }])
    );
    assert!(!hold.returned(), "draining ended the call");

    hold.release();
    let deadline = Instant::now() + CALL_PATIENCE;
    while draining(&rig, &runner)["calls"] != json!([]) {
        assert!(
            Instant::now() < deadline,
            "the turn's end was never recorded"
        );
        step(&runner).unwrap();
        std::thread::yield_now();
    }
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.claude.calls().len(), 1, "nothing started after it");
}

#[test]
fn a_pause_sets_no_flag_for_a_work_item_with_no_turn_in_flight() {
    let (rig, runner) = seven_due("shep");
    // Issue 7's first turn is due, not running: nothing for a tool result to end.
    std::fs::create_dir_all(rig.build_7()).unwrap();
    let drained = rig.ask(&runner, "drain", Some("pause"));
    assert_eq!(drained["draining"]["at_tool"], json!(null), "{drained}");
    assert!(
        !Pause::in_build(&rig.build_7()).is_requested(),
        "a flag with no turn to read it would end the next one"
    );
}

#[test]
fn a_restart_ends_draining() {
    let (rig, runner) = seven_due("rotom");
    rig.ask(&runner, "drain", None);
    drop(runner);
    let runner = rig.open().unwrap();
    assert_eq!(draining(&rig, &runner), json!(null));
    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(&runner).unwrap();
    assert_eq!(rig.claude.calls().len(), 1);
}

#[test]
fn a_drained_runner_starts_no_review_call() {
    let (rig, runner) = seven_due("shep");
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
    step(&runner).unwrap(); // the worker's first turn
    rig.ask(&runner, "drain", None);
    assert_eq!(step(&runner).unwrap(), None);
    assert!(rig.reviewer.seen().is_empty(), "the local round waits");

    rig.ask(&runner, "undrain", None);
    step(&runner).unwrap(); // round 1, qwen
    assert_eq!(rig.reviewer.seen().len(), 1);
    rig.ask(&runner, "drain", None);
    assert_eq!(step(&runner).unwrap(), None);
    let reviews = |rig: &Rig| {
        let calls = rig.claude.all_calls().into_iter();
        calls.filter(|c| c.role == Role::Reviewer).count()
    };
    assert_eq!(reviews(&rig), 0, "the reviewer's session waits");
    rig.ask(&runner, "undrain", None);
    step(&runner).unwrap(); // round 2, claude
    assert_eq!(reviews(&rig), 1);
}

#[test]
fn a_drained_runner_still_merges() {
    let (rig, runner, head) = Rig::parked("reactmap");
    rig.ask(&runner, "drain", None);
    rig.ask(&runner, "rule", Some("71 yes"));
    for _ in 0..3 {
        if !rig.forge.merges().is_empty() {
            break;
        }
        step(&runner).unwrap();
        rig.clock.advance(CHECKS_SETTLE);
    }
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

#[test]
fn drain_and_undrain_take_no_params() {
    let rig = Rig::new("xilriws");
    let runner = rig.open().unwrap();
    for action in ["drain", "undrain"] {
        assert_eq!(
            rig.ask(&runner, action, Some("now")),
            json!({ "error": format!("`{action}` takes no params") })
        );
    }
    assert_eq!(draining(&rig, &runner), json!(null));
}

#[test]
fn a_drained_runner_does_not_wake_the_project_manager() {
    let rig = Rig::new("eevee");
    rig.edit_settings(|s| {
        let listed = "\nimplementers = [\"sonnet-high\"]\n";
        assert!(s.contains(listed), "the example's implementers moved");
        s.replace(listed, "\nimplementers = [\"sonnet-high\"]\npm = \"pm\"\n")
    });
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "drain", None)["draining"]["ceiling"],
        3600,
        "the turn ceiling, longer than the project manager's"
    );
    rig.ask(&runner, "tell", Some("Hold #12"));
    let woken = |rig: &Rig| {
        let calls = rig.claude.all_calls().into_iter();
        calls.filter(|c| c.role == Role::Pm).count()
    };
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(woken(&rig), 0, "the wake waits");

    rig.ask(&runner, "undrain", None);
    rig.claude.script([Scripted::Say("{\"pick\": null}")]);
    step(&runner).unwrap();
    assert_eq!(woken(&rig), 1);
}

// A `drain pause` answered while a pass holds the runner's lock sets its flag
// at once, and leaves the pause for the pass to set at its next lock.
#[test]
fn a_drain_answered_while_a_pass_holds_the_lock_leaves_its_ask_for_the_pass() {
    let (rig, runner) = seven_due("koji");
    std::fs::create_dir_all(rig.build_7()).unwrap();
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the turn never began");

    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let mut pass = runner.lock().unwrap();
    let asking = {
        let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
        std::thread::spawn(move || answer(&runner, &postings, "drain", Some("pause")))
    };
    let reply: Value = serde_json::from_str(&asking.join().unwrap()).expect("a JSON reply");
    assert_eq!(
        reply["draining"]["calls"],
        json!([{ "issue": "7", "role": "worker" }]),
        "{reply}"
    );
    assert!(
        !Pause::in_build(&rig.build_7()).is_requested(),
        "the pause was set while the pass still held the lock"
    );

    pass.take_posted();
    assert!(
        Pause::in_build(&rig.build_7()).is_requested(),
        "the drain pause never reached the turn in flight"
    );
    hold.release();
}

// An `undrain` answered with the lock free acts at once, so it takes up a
// `drain` that landed while a pass held the lock. Left for the next pass, that
// drain pauses the turns this one let go of, and the drain wins last.
#[test]
fn an_undrain_takes_up_a_drain_that_landed_while_a_pass_held_the_lock() {
    let (rig, runner) = seven_due("koji");
    std::fs::create_dir_all(rig.build_7()).unwrap();
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the turn never began");

    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let held = runner.lock().unwrap();
    let asking = {
        let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
        std::thread::spawn(move || answer(&runner, &postings, "drain", Some("pause")))
    };
    asking.join().unwrap();
    drop(held);
    assert!(
        !Pause::in_build(&rig.build_7()).is_requested(),
        "the posted drain set the pause while a pass held the lock"
    );

    // The lock is free, so this `undrain` is answered by the runner.
    rig.ask(&runner, "undrain", None);
    assert!(!postings.draining(), "the undrain left the runner draining");

    let mut pass = runner.lock().unwrap();
    pass.take_posted();
    assert!(
        !Pause::in_build(&rig.build_7()).is_requested(),
        "the next pass paused the turn the undrain had let go of"
    );
    assert!(!postings.draining(), "the next pass drained again");
    drop(pass);
    hold.release();
}

// A `drain` answered with the lock free acts at once, so it takes up an
// `undrain` that landed while a pass held the lock. Left for the next pass,
// that undrain withdraws the pause this one set, and the undrain wins last.
#[test]
fn a_drain_takes_up_an_undrain_that_landed_while_a_pass_held_the_lock() {
    let (rig, runner) = seven_due("koji");
    std::fs::create_dir_all(rig.build_7()).unwrap();
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the turn never began");

    rig.ask(&runner, "drain", Some("pause"));
    assert!(
        Pause::in_build(&rig.build_7()).is_requested(),
        "the pause never reached the turn in flight"
    );

    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let held = runner.lock().unwrap();
    let asking = {
        let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
        std::thread::spawn(move || answer(&runner, &postings, "undrain", None))
    };
    asking.join().unwrap();
    drop(held);
    assert!(
        !postings.draining(),
        "the posted undrain did not let go at once"
    );
    assert!(
        Pause::in_build(&rig.build_7()).is_requested(),
        "the posted undrain withdrew the pause itself"
    );

    // The lock is free, so this `drain` is answered by the runner.
    rig.ask(&runner, "drain", None);
    assert!(postings.draining(), "the drain did not hold new calls back");

    let mut pass = runner.lock().unwrap();
    pass.take_posted();
    assert!(
        postings.draining(),
        "the next pass undrained what this drained"
    );
    assert!(
        Pause::in_build(&rig.build_7()).is_requested(),
        "the next pass withdrew the pause the drain set"
    );
    drop(pass);
    hold.release();
}

#[test]
fn every_harness_whose_attached_session_holds_a_handover_has_a_role() {
    let held: Vec<_> = crate::settings::Harness::ALL
        .into_iter()
        .filter(|h| h.facts().holds_handover_at_attach.is_some())
        .collect();
    let roles: Vec<_> = super::ATTACHED.iter().map(|(h, _)| *h).collect();

    assert!(held.iter().all(|h| roles.contains(h)), "{held:?} {roles:?}");
    assert_eq!(roles.len(), held.len());
}
