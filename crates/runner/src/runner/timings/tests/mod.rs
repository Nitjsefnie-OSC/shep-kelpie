//! What the runner charges to each phase, read through `status` and `timings`

use std::sync::Mutex;

use serde_json::Value;

use crate::runner::{Runner, step};
use crate::state::{Finished, ProjectState, StateStore};
use crate::test::{CALL_PATIENCE, Hold, Rig, ticket};
use crate::work_item::{Seconds, TimingPhase};

mod charging;
mod phases;
mod totals;

// Read under the lock: `status` asked through a trigger is answered from the
// copy last posted while a pass holds the lock, and that copy was charged
// before the test moved the clock, so it counts none of the seconds since.
fn timings(_rig: &Rig, runner: &Mutex<Runner>) -> Value {
    let status = serde_json::to_value(runner.lock().unwrap().status()).expect("status serializes");
    status["work_item"]["timings"].clone()
}

fn secs(timings: &Value, phase: &str) -> u64 {
    timings["seconds"][phase]
        .as_u64()
        .expect("every phase is there")
}

fn summed(timings: &Value) -> u64 {
    timings["seconds"]
        .as_object()
        .unwrap()
        .values()
        .map(|s| s.as_u64().unwrap())
        .sum()
}

fn assert_sums(timings: &Value) {
    assert_eq!(
        summed(timings),
        timings["wall"].as_u64().unwrap(),
        "{timings}"
    );
}

// Runs the step that begins a call and holds the call. Runs `during`, moves
// the clock, and returns what `status` says.
fn held_while(
    rig: &Rig,
    runner: &Mutex<Runner>,
    hold: &Hold,
    seconds: u64,
    during: impl FnOnce(),
) -> Value {
    std::thread::scope(|scope| {
        let call = scope.spawn(|| step(runner));
        assert!(hold.entered(CALL_PATIENCE), "the call never began");
        during();
        rig.clock.advance(seconds);
        let read = timings(rig, runner);
        hold.release();
        call.join().unwrap().unwrap();
        read
    })
}

fn read_while_held(rig: &Rig, runner: &Mutex<Runner>, hold: &Hold, seconds: u64) -> Value {
    held_while(rig, runner, hold, seconds, || {})
}

fn finished(issue: u64, merged: bool, pairs: &[(TimingPhase, u64)]) -> Finished {
    let seconds = Seconds::of(pairs);
    Finished {
        issue: ticket(issue),
        title: format!("Issue {issue}"),
        pull_request: Some(issue + 60),
        merged,
        closed: false,
        at: crate::ports::Timestamp(Rig::EPOCH + issue),
        wall: seconds.total(),
        seconds,
        spend: None,
    }
}

// A rig whose state file already holds these finished work items
fn with_history(history: Vec<Finished>) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("koji");
    let mut state = ProjectState::new();
    state.history = history;
    let file = rig.paths().state;
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    StateStore::new(file).save(&state).unwrap();
    let runner = rig.open().unwrap();
    (rig, runner)
}
