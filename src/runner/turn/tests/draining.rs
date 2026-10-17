//! A `drain` that lands while a pass holds the runner's lock
//!
//! A pass checks that it drains just before it saves the turn it starts. Every
//! forge call has to sit outside that check and the call it starts, since a
//! `drain` answered from the posted copy sets its flag there and waits for
//! nothing.

use std::sync::Arc;

use super::*;
use crate::runner::answer;
use crate::runner::flight::{Pass, advance};
use crate::test::CALL_PATIENCE;

// Preparing a worker turn mints its GitHub token with the runner's lock held.
// A `drain` answered from the posted copy in that time lands after the pass has
// checked whether it drains and before it saves the turn as running, so the
// turn stays due rather than starting past the drain.
#[test]
fn a_drain_that_lands_while_a_turn_is_prepared_starts_no_call() {
    let rig = Rig::new("koji");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let mut mint = rig.github.hold_next_worker_token();

    let pass = std::thread::scope(|scope| {
        let pass = scope.spawn(|| advance(&runner));
        assert!(
            mint.waiting(CALL_PATIENCE),
            "the turn never reached its token mint"
        );
        let asking = {
            let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
            scope.spawn(move || answer(&runner, &postings, "drain", Some("pause")))
        };
        let reply: serde_json::Value =
            serde_json::from_str(&asking.join().unwrap()).expect("a JSON reply");
        assert!(
            postings.draining(),
            "the drain did not hold new calls back: {reply}"
        );
        mint.release();
        pass.join().expect("the pass thread")
    });
    assert!(
        matches!(pass, Ok(Pass::Idle)),
        "the pass started a call past the drain: {pass:?}"
    );

    assert_eq!(rig.claude.all_calls(), [], "the worker started anyway");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["held_up"]["because"],
        "the runner is draining, so no turn starts",
        "the turn does not say it waits on the drain"
    );
}

// A pass reads `main`'s workflow runs with the runner's lock held, and that
// read waits its turn behind the login's pace. Read before any work item is
// stepped, the step that follows checks the drain after it. Read after the
// step chose its call, the call started past a `drain` answered in the read.
#[test]
fn a_drain_that_lands_while_mains_workflows_are_read_starts_no_call() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let runner = Arc::new(runner);
    let postings = runner.lock().unwrap().postings();
    let read = rig.forge.hold_next_main_runs_read();

    let pass = std::thread::scope(|scope| {
        let pass = scope.spawn(|| advance(&runner));
        assert!(
            read.waiting(CALL_PATIENCE),
            "the pass never reached the read of `main`'s workflows"
        );
        let asking = {
            let (runner, postings) = (Arc::clone(&runner), Arc::clone(&postings));
            scope.spawn(move || answer(&runner, &postings, "drain", Some("pause")))
        };
        let reply: serde_json::Value =
            serde_json::from_str(&asking.join().unwrap()).expect("a JSON reply");
        assert!(
            postings.draining(),
            "the drain did not hold new calls back: {reply}"
        );
        drop(read);
        pass.join().expect("the pass thread")
    });
    assert!(
        matches!(pass, Ok(Pass::Idle)),
        "the pass started a call past the drain: {pass:?}"
    );

    assert_eq!(rig.claude.all_calls(), [], "the worker started anyway");
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["held_up"]["because"],
        "the runner is draining, so no turn starts",
        "the turn does not say it waits on the drain"
    );
}
