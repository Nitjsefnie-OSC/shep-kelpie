use std::sync::Mutex;

use crate::github::ApiError;
use crate::ports::{Cooldown, Cost, Timestamp, Usage};
use crate::runner::{Runner, step};
use crate::test::{Rig, Scripted};

// A first turn for issue 7, which asks for its worker's token.
fn start(rig: &Rig, runner: &Mutex<Runner>) {
    rig.ask(runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(runner).unwrap();
}

fn refused(retry_after: Option<u64>, reset: Option<Timestamp>) -> ApiError {
    ApiError::RateLimited(Cooldown { retry_after, reset })
}

#[test]
fn a_mint_refused_with_a_retry_after_waits_that_long_not_a_fixed_thirty_seconds() {
    let rig = Rig::new("koji");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.github.refuse_workers(refused(Some(120), None));

    start(&rig, &runner);
    assert!(rig.claude.seen().is_empty(), "no turn without a token");

    // GitHub took the token again, but it said to wait two minutes.
    rig.github.accept_workers();
    rig.clock.advance(60);
    step(&runner).unwrap();
    assert!(rig.claude.seen().is_empty(), "still told to wait");

    rig.clock.advance(61);
    step(&runner).unwrap();
    assert_eq!(rig.claude.seen().len(), 1, "the turn began once it passed");
}

#[test]
fn a_mint_refused_with_a_reset_waits_until_the_reset() {
    let rig = Rig::new("koji");
    rig.with_app();
    let runner = rig.open().unwrap();
    let reset = Timestamp(Rig::EPOCH + 300);
    rig.github.refuse_workers(refused(None, Some(reset)));

    start(&rig, &runner);
    rig.github.accept_workers();
    rig.clock.advance(299);
    step(&runner).unwrap();
    assert!(rig.claude.seen().is_empty());

    rig.clock.advance(1);
    step(&runner).unwrap();
    assert_eq!(rig.claude.seen().len(), 1);
}
