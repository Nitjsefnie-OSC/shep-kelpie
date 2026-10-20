//! A Codex turn's context size and the compaction kelpie asks of it
//!
//! `fixtures/codex-exec-compacted.jsonl` and `fixtures/codex-rollout-compacted.jsonl`
//! are written from the shapes in Codex's source at `rust-v0.159.3`, not
//! recorded, until the maintainer's live run records them (#771).

use super::tests::{FRESH, THREAD, WORKER_ID, World, id, stand_in, strings};
use super::*;
use crate::ports::{COMPACT, Role};

/// A resumed turn that compacted first, in the session `FRESH` began
const COMPACTED: &str = include_str!("../../../../../fixtures/codex-exec-compacted.jsonl");
/// The session's rollout after that turn: the first turn's request, the
/// compaction's, the turn's own, and a rate-limit update with no usage
const ROLLOUT: &str = include_str!("../../../../../fixtures/codex-rollout-compacted.jsonl");

// Puts `rollout` where Codex keeps the rollout of `THREAD` in the call's home.
fn rollout(w: &World, call: &AgentCall, rollout: &str) {
    let folder = Files::of(call).home.join("sessions/2026/10/13");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("rollout-2026-10-13T09-00-00-{THREAD}.jsonl")),
        rollout,
    )
    .unwrap();
    assert!(w.path("worker").is_dir());
}

#[test]
fn a_turn_s_context_is_the_last_token_count_events_last_request_input() {
    let w = World::new();
    let cli = stand_in(&w, COMPACTED);
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    cli.prepare(&call).unwrap();
    rollout(&w, &call, ROLLOUT);

    let reply = cli.run(&call, &Ending::default()).unwrap();

    // The turn's own request after the compaction, not the session's running total.
    assert_eq!(reply.context, Some(1523));
}

#[test]
fn a_rollout_with_no_token_count_event_or_no_file_reports_no_context() {
    let w = World::new();
    let cli = stand_in(&w, COMPACTED);
    let call = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    cli.prepare(&call).unwrap();

    let no_file = cli.run(&call, &Ending::default()).unwrap();
    assert_eq!(no_file.context, None);

    // A rollout that held only a session's start and a usage-less rate-limit update.
    let events = ROLLOUT
        .lines()
        .filter(|l| !l.contains("\"last_token_usage\""))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(events.lines().count(), 3, "{events}");
    rollout(&w, &call, &events);
    let again = w.call(Role::Worker, Session::Resume(id(WORKER_ID)));
    let no_event = cli.run(&again, &Ending::default()).unwrap();
    assert_eq!(no_event.context, None);
}

#[test]
fn a_compact_window_sets_the_auto_compact_limit_and_scope_and_zero_leaves_them_out() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    call.compact_window = std::num::NonZeroU32::new(200_000);

    let with = strings(&call, None, None);

    assert!(
        with.windows(2)
            .any(|a| a == ["-c", "model_auto_compact_token_limit=200000"]),
        "{with:?}"
    );
    assert!(
        with.windows(2)
            .any(|a| a == ["-c", "model_auto_compact_token_limit_scope=\"total\""]),
        "{with:?}"
    );

    // The runner passes no window as zero, which the call holds as `None`.
    call.compact_window = std::num::NonZeroU32::new(0);
    let without = strings(&call, None, None);
    assert!(
        !without.iter().any(|a| a.contains("auto_compact")),
        "{without:?}"
    );
}

// The session `FRESH` began, with `ROLLOUT` as the rollout its last turn left.
fn after_a_turn(w: &World, cli: &CodexCli) -> AgentCall {
    let first = w.call(Role::Worker, Session::New(id(WORKER_ID)));
    cli.prepare(&first).unwrap();
    rollout(w, &first, ROLLOUT);
    cli.run(&first, &Ending::default()).unwrap();
    std::fs::remove_file(w.path("argv")).unwrap();
    w.call(Role::Worker, Session::Resume(id(WORKER_ID)))
}

#[test]
fn a_compact_call_runs_no_codex_and_records_the_context_it_read() {
    let w = World::new();
    let cli = stand_in(&w, FRESH);
    let mut call = after_a_turn(&w, &cli);
    call.prompt = COMPACT.into();

    let reply = cli.run(&call, &Ending::default()).unwrap();

    assert!(!w.path("argv").exists(), "codex ran");
    assert_eq!(reply.text, "");
    assert_eq!(reply.usage, Usage::default());
    assert_eq!(reply.context, None);
    let recorded = thread(&Files::of(&call), &id(WORKER_ID)).unwrap().unwrap();
    assert_eq!(recorded.context, Some(1523));
}

#[test]
fn the_turn_after_a_compact_call_passes_the_lower_of_the_window_and_the_recorded_context_less_one()
{
    let w = World::new();
    let cli = stand_in(&w, FRESH);
    let mut compact = after_a_turn(&w, &cli);
    compact.prompt = COMPACT.into();
    cli.run(&compact, &Ending::default()).unwrap();
    let limit = |w: &World| {
        std::fs::read_to_string(w.path("argv"))
            .unwrap()
            .lines()
            .find_map(|a| {
                a.strip_prefix("model_auto_compact_token_limit=")
                    .map(str::to_owned)
            })
    };

    // The session held 1523 tokens, so a limit of 1522 is already reached.
    let mut next = w.call(Role::Worker, Session::Resume(id(WORKER_ID)));
    next.compact_window = std::num::NonZeroU32::new(200_000);
    cli.run(&next, &Ending::default()).unwrap();
    assert_eq!(limit(&w).as_deref(), Some("1522"));

    // That turn ran, so the one after it holds only the window.
    cli.run(&next, &Ending::default()).unwrap();
    assert_eq!(limit(&w).as_deref(), Some("200000"));

    // A window under the context is the lower, and a session not compacted has the window.
    cli.run(&compact, &Ending::default()).unwrap();
    next.compact_window = std::num::NonZeroU32::new(1_000);
    cli.run(&next, &Ending::default()).unwrap();
    assert_eq!(limit(&w).as_deref(), Some("1000"));
}

// The running total after the turn is 36,989 input (28,416 cached) and 952
// output, against 17,711 (10,752) and 44 before it. The compaction's own
// request (17,755 input, 17,664 of them cached, 812 output) is inside that
// total, so the turn's usage holds it with no count of its own: 91 uncached
// input of the compaction and 1,523 of the turn, and 812 + 96 output.
#[test]
fn a_turn_that_compacted_reports_the_usage_of_the_compaction_in_its_total() {
    let w = World::new();
    let cli = stand_in(&w, FRESH);
    let call = after_a_turn(&w, &cli);
    let cli = stand_in(&w, COMPACTED);

    let reply = cli.run(&call, &Ending::default()).unwrap();

    assert_eq!(
        reply.usage,
        Usage {
            input: 1614,
            cache_write: 0,
            cache_write_5m: 0,
            cache_read: 17_664,
            output: 908,
        }
    );
}
