//! A worker turn that a `drain` lands on while its turn is being prepared

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
