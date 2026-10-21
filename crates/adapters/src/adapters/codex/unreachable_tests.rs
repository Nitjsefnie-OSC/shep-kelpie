//! Codex's error text read as a provider that cannot be reached or is overloaded

use super::tests::{FRESH, WORKER_ID, World, id, stand_in};
use super::*;
use crate::ports::Role;

const STATUS_503: &str = include_str!("../../../../../fixtures/codex-exec-503.jsonl");
const STATUS_502: &str = include_str!("../../../../../fixtures/codex-exec-502.jsonl");
const HIGH_DEMAND: &str = include_str!("../../../../../fixtures/codex-exec-high-demand.jsonl");
const AT_CAPACITY: &str = include_str!("../../../../../fixtures/codex-exec-at-capacity.jsonl");
const STREAM_DISCONNECTED: &str =
    include_str!("../../../../../fixtures/codex-exec-stream-disconnected.jsonl");
const CONNECTION_FAILED: &str =
    include_str!("../../../../../fixtures/codex-exec-connection-failed.jsonl");
const RETRY_LIMIT_429: &str =
    include_str!("../../../../../fixtures/codex-exec-retry-limit-429.jsonl");
const BAD_REQUEST: &str = include_str!("../../../../../fixtures/codex-exec-bad-request.jsonl");
const USAGE_LIMIT: &str = include_str!("../../../../../fixtures/codex-exec-usage-limit.jsonl");
const RECONNECTED: &str = include_str!("../../../../../fixtures/codex-exec-reconnected.jsonl");

// A first worker turn that printed `stdout`, as the runner's call reads it.
fn first_turn(stdout: &str) -> Result<AgentReply, AgentError> {
    let w = World::new();
    let cli = stand_in(&w, stdout);
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    cli.prepare(&call).unwrap();
    cli.run(&call, &Ending::default())
}

#[test]
fn a_provider_that_cannot_be_reached_or_is_overloaded_is_unreachable() {
    for (case, stdout) in [
        ("a 503", STATUS_503),
        ("a 502", STATUS_502),
        (
            "a 504",
            &STATUS_503.replace("503 Service Unavailable", "504 Gateway Timeout"),
        ),
        ("a 500", HIGH_DEMAND),
        ("a 503 from an overloaded server", AT_CAPACITY),
        ("stream ended early", STREAM_DISCONNECTED),
        ("connection failed", CONNECTION_FAILED),
        (
            "connection refused",
            &CONNECTION_FAILED.replace("error sending request", "Connection refused"),
        ),
        (
            "connection reset",
            &CONNECTION_FAILED.replace("error sending request", "Connection reset by peer"),
        ),
    ] {
        let err = first_turn(stdout).unwrap_err();
        assert!(
            matches!(&err, AgentError::Unreachable(Harness::Codex, why) if !why.is_empty()),
            "{case}: {err:?}"
        );
    }
}

#[test]
fn the_detail_of_an_unreachable_provider_is_codexs_own_message() {
    let Err(AgentError::Unreachable(Harness::Codex, why)) = first_turn(STATUS_503) else {
        panic!("a 503 was not unreachable");
    };
    assert_eq!(
        why,
        "unexpected status 503 Service Unavailable: stub 503, url: http://127.0.0.1:18700/v1/responses, request id: req-stub-30"
    );
}

#[test]
fn any_other_error_fails() {
    for (case, stdout) in [
        ("usage limit", USAGE_LIMIT),
        ("a 400", BAD_REQUEST),
        ("a 429", RETRY_LIMIT_429),
        (
            "a 429 whose text mentions a reset connection",
            &RETRY_LIMIT_429.replace("Too Many Requests", "Too Many Requests: connection reset"),
        ),
        (
            "a 400 whose text mentions a refused connection",
            &BAD_REQUEST.replace("Unsupported parameter", "upstream connection refused"),
        ),
    ] {
        let err = first_turn(stdout).unwrap_err();
        assert!(
            matches!(err, AgentError::Failed(Harness::Codex, _)),
            "{case}: {err:?}"
        );
    }
}

#[test]
fn a_turn_that_ends_well_after_reconnects_succeeds() {
    let reply = first_turn(RECONNECTED).unwrap();
    assert_eq!(reply.text, "done");
    // The recorded turn that did not reconnect says the same.
    assert_eq!(first_turn(FRESH).unwrap().text, "done");
}

#[test]
fn a_first_turn_that_found_no_host_is_tried_again_as_a_new_session() {
    let w = World::new();
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    let down = stand_in(&w, AT_CAPACITY);
    down.prepare(&call).unwrap();
    assert!(matches!(
        down.run(&call, &Ending::default()),
        Err(AgentError::Unreachable(Harness::Codex, _))
    ));
    // The runner replays the call it kept, a new session, and no thread was
    // recorded for the failed turn, so the retry is not refused as a second start.
    let up = stand_in(&w, FRESH);
    let reply = up.run(&call, &Ending::default()).unwrap();
    assert_eq!(reply.text, "done");
    // The session resumes from the retry on.
    let next = w.call(Role::Worker, Session::Resume(id(WORKER_ID)));
    let resumed = stand_in(&w, FRESH).run(&next, &Ending::default()).unwrap();
    assert_eq!(resumed.text, "done");
}
