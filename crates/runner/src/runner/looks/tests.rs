//! How often a pass reads the forge, through the runner's stand-ins

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::ports::{CallPacer, Clock, LocalPace, Timestamp};
use crate::runner::{BOARD_POLL, Pass, StepReport, advance, step};
use crate::test::Rig;
use crate::test::ticket;

// The first wait of a failed step, in seconds
const RETRY_FIRST: u64 = 15;

// The loop's pass after a wake, whatever woke it
fn wake(runner: &std::sync::Mutex<crate::runner::Runner>) -> Option<StepReport> {
    match advance(runner).unwrap() {
        Pass::Report(report) => Some(report),
        Pass::Idle | Pass::Started => None,
    }
}

#[test]
fn many_wakes_in_a_minute_read_the_board_once() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    let before = rig.forge.board_reads();
    for _ in 0..30 {
        wake(&runner);
    }
    assert_eq!(rig.forge.board_reads(), before + 1);

    rig.clock.advance(BOARD_POLL.as_secs() - 1);
    wake(&runner);
    assert_eq!(
        rig.forge.board_reads(),
        before + 1,
        "read inside the minute"
    );
    rig.clock.advance(1);
    for _ in 0..30 {
        wake(&runner);
    }
    assert_eq!(rig.forge.board_reads(), before + 2);
}

#[test]
fn a_work_item_ending_lets_the_board_be_read_at_once() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    // Draining, so #7 starts no call while it holds the one slot.
    rig.ask(&runner, "drain", None);
    rig.ask(&runner, "add", Some("7"));
    wake(&runner);
    let before = rig.forge.board_reads();
    rig.forge.list_ready(8, false);
    rig.ask(&runner, "drop", Some("7"));

    let report = wake(&runner);

    assert_eq!(rig.forge.board_reads(), before + 1);
    assert!(
        matches!(report, Some(StepReport::Dispatched { issue, .. }) if issue == ticket(8)),
        "{report:?}"
    );
}

#[test]
fn a_failing_gate_is_retried_on_a_growing_wait_that_a_success_ends() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    rig.forge.set_down(true);
    let failed =
        |report| matches!(report, Some(StepReport::GateFailed { issue, .. }) if issue == ticket(7));
    assert!(failed(step(&runner).unwrap()));

    for wait in [15, 30, 60] {
        let asked = rig.forge.asked();
        assert_eq!(step(&runner).unwrap(), None);
        rig.clock.advance(wait - 1);
        assert_eq!(step(&runner).unwrap(), None);
        assert_eq!(
            rig.forge.asked(),
            asked,
            "the forge was asked inside a wait"
        );
        rig.clock.advance(1);
        assert!(failed(step(&runner).unwrap()), "no retry after {wait}s");
        assert!(rig.forge.asked() > asked);
    }

    rig.forge.set_down(false);
    rig.clock.advance(120);
    assert_eq!(step(&runner).unwrap(), None, "CI is read, and pending");
    rig.forge.set_down(true);
    assert!(failed(step(&runner).unwrap()));
    rig.clock.advance(15);
    assert!(
        failed(step(&runner).unwrap()),
        "the wait did not start over"
    );
}

// What `status` shows of the oldest work item's failing step, if it does
fn failing(rig: &Rig, runner: &std::sync::Mutex<crate::runner::Runner>) -> Option<Value> {
    rig.ask(runner, "status", None)["work_item"]
        .get("failing")
        .cloned()
}

fn gate_failed(report: Option<StepReport>) -> String {
    match report {
        Some(StepReport::GateFailed { issue, reason }) if issue == ticket(7) => reason,
        other => panic!("#7's gate did not fail: {other:?}"),
    }
}

#[test]
fn status_shows_a_step_failing_in_a_row_from_its_second_failure_with_its_reason() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    rig.forge.set_down(true);
    let first = rig.clock.now();

    gate_failed(step(&runner).unwrap());
    assert_eq!(failing(&rig, &runner), None, "one failure is no streak");

    rig.clock.advance(RETRY_FIRST);
    let reason = gate_failed(step(&runner).unwrap());
    assert_eq!(
        failing(&rig, &runner),
        Some(json!({ "since": first.0, "failures": 2, "reason": reason }))
    );

    rig.clock.advance(2 * RETRY_FIRST);
    gate_failed(step(&runner).unwrap());
    let shown = failing(&rig, &runner).unwrap();
    assert_eq!(
        (&shown["since"], &shown["failures"]),
        (&json!(first.0), &json!(3))
    );
}

#[test]
fn a_step_that_does_not_fail_clears_its_failing_in_status() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    rig.forge.set_down(true);
    gate_failed(step(&runner).unwrap());
    rig.clock.advance(RETRY_FIRST);
    gate_failed(step(&runner).unwrap());
    assert!(failing(&rig, &runner).is_some());

    rig.forge.set_down(false);
    rig.clock.advance(2 * RETRY_FIRST);
    assert_eq!(step(&runner).unwrap(), None, "CI is read, and pending");

    assert_eq!(failing(&rig, &runner), None);
}

#[test]
fn a_runner_s_forge_calls_land_in_kelpie_s_forge_ledger_under_its_project() {
    use crate::forge_ledger::{self, Ended, Kind};

    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    wake(&runner);
    rig.forge
        .set_used_up(Some(Timestamp(rig.clock.now().0 + 1800)));
    rig.clock.advance(BOARD_POLL.as_secs());
    wake(&runner);
    rig.clock.advance(BOARD_POLL.as_secs());
    wake(&runner);

    let lines = forge_ledger::read(&rig.paths().kelpie_home.join(forge_ledger::FILE)).unwrap();
    assert!(lines.iter().all(|line| line.project == "acme"), "{lines:?}");
    let board = |ended| {
        lines
            .iter()
            .filter(move |l| l.kind == Kind::ReadyIssues && l.ended == ended)
    };
    assert!(board(Ended::Ok).count() >= 1, "{lines:?}");
    assert_eq!(board(Ended::UsedUp).count(), 1, "{lines:?}");
}

#[test]
fn a_used_up_rate_limit_holds_every_forge_call_until_it_resets_and_says_so_once() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    runner.lock().unwrap().take_notes();
    let reset = Timestamp(rig.clock.now().0 + 1800);
    rig.forge.set_used_up(Some(reset));

    assert!(matches!(
        wake(&runner),
        Some(StepReport::BoardFailed { .. })
    ));
    let asked = rig.forge.asked();
    let notes = runner.lock().unwrap().take_notes();
    assert_eq!(notes.len(), 1, "{notes:?}");
    let until = jiff::Timestamp::from_second(i64::try_from(reset.0).unwrap()).unwrap();
    assert!(
        notes[0].ends_with(&format!("makes no forge call until {until}")),
        "{}",
        notes[0]
    );
    assert_eq!(
        rig.ask(&runner, "status", None)["forge_held_until"],
        reset.0
    );

    let refused = rig.ask(&runner, "add", Some("8"));
    assert!(
        refused["error"]
            .as_str()
            .unwrap()
            .contains("rate limit is used up until"),
        "{refused}"
    );
    for _ in 0..29 {
        rig.clock.advance(60);
        wake(&runner);
    }
    rig.clock.advance(59);
    wake(&runner);
    assert_eq!(rig.forge.asked(), asked, "a held call reached the forge");
    assert_eq!(runner.lock().unwrap().take_notes(), Vec::<String>::new());

    rig.forge.set_down(false);
    rig.clock.advance(1);
    assert_eq!(wake(&runner), None);
    assert!(rig.forge.asked() > asked);
    assert_eq!(
        rig.ask(&runner, "status", None).get("forge_held_until"),
        None
    );
}

#[test]
fn a_hold_another_runner_put_on_the_login_stops_this_runners_forge_calls() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    runner.lock().unwrap().take_notes();
    let asked = rig.forge.asked();
    // The other runner's used-up answer, put on the pace both reach.
    let until = Timestamp(rig.clock.now().0 + 300);
    rig.pace.hold(until);

    wake(&runner);

    assert_eq!(rig.forge.asked(), asked, "a held call reached the forge");
    assert_eq!(
        rig.ask(&runner, "status", None)["forge_held_until"],
        until.0
    );
    rig.clock.advance(300);
    wake(&runner);
    assert!(rig.forge.asked() > asked);
}

#[test]
fn a_runner_s_calls_past_the_logins_pace_wait_and_no_minute_in_the_ledger_is_over_the_cap() {
    use crate::forge_ledger;

    const CAP: usize = 4;
    let mut rig = Rig::new("acme");
    rig.pace = Arc::new(LocalPace::with(CallPacer::new(CAP, 2)));
    let runner = rig.open().unwrap();
    for _ in 0..4 {
        rig.clock.advance(BOARD_POLL.as_secs());
        wake(&runner);
    }

    let lines = forge_ledger::read(&rig.paths().kelpie_home.join(forge_ledger::FILE)).unwrap();
    assert!(lines.len() > CAP, "the runner made calls enough: {lines:?}");
    let mut minutes = std::collections::BTreeMap::<u64, usize>::new();
    for line in &lines {
        *minutes.entry(line.at.0 / 60).or_default() += 1;
    }
    assert!(minutes.values().all(|calls| *calls <= CAP), "{minutes:?}");
}

#[test]
fn the_hold_is_told_in_turn_with_the_notes_after_it() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    runner.lock().unwrap().take_notes();
    rig.forge
        .set_used_up(Some(Timestamp(rig.clock.now().0 + 1800)));
    wake(&runner);
    // A stray file in the answers folder is a note of its own, made with no forge call.
    let answers = rig.paths().answers;
    std::fs::create_dir_all(&answers).unwrap();
    std::fs::write(answers.join("stray"), "").unwrap();
    wake(&runner);

    let notes = runner.lock().unwrap().take_notes();

    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(
        notes[0].starts_with("the forge's rate limit is used up"),
        "{notes:?}"
    );
    assert!(notes[1].contains("stray"), "{notes:?}");
}

#[test]
fn a_rate_limit_whose_reset_the_forge_cannot_say_holds_for_ten_minutes() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    rig.forge.set_used_up(None);

    wake(&runner);

    let held = rig.ask(&runner, "status", None)["forge_held_until"].clone();
    assert_eq!(held, rig.clock.now().0 + 600);
}

// After an idle pass, nothing the runner waits on is due already, so the
// loop sleeps rather than spins.

#[test]
fn a_board_shut_by_a_full_slot_sets_no_wait_past_its_last_read() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    wake(&runner);
    rig.ask(&runner, "drain", None);
    rig.ask(&runner, "add", Some("7"));
    rig.clock.advance(3 * BOARD_POLL.as_secs());

    assert_eq!(wake(&runner), None);

    assert_eq!(runner.lock().unwrap().next_look(), None);
}

#[test]
fn a_hold_longer_than_a_minute_waits_for_its_end_not_the_board_read() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    rig.forge
        .set_used_up(Some(Timestamp(rig.clock.now().0 + 1800)));
    wake(&runner);
    rig.clock.advance(BOARD_POLL.as_secs() + 1);

    assert_eq!(wake(&runner), None);

    let wait = runner.lock().unwrap().next_look();
    assert_eq!(wait, Some(Duration::from_secs(1800 - 61)));
}

#[test]
fn the_wait_of_a_work_item_that_ended_sets_no_wait() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    rig.forge.set_down(true);
    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::GateFailed { issue, .. }) if issue == ticket(7)
    ));
    rig.forge.set_down(false);
    rig.ask(&runner, "drop", Some("7"));
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(20);

    let wait = runner.lock().unwrap().next_look();

    assert_eq!(wait, Some(Duration::from_secs(BOARD_POLL.as_secs() - 20)));
}

// A work item takes the one slot, which shuts the board, and then frees it,
// with the forge up for the work item to be added and down again after
fn take_and_free_the_slot(rig: &Rig, runner: &std::sync::Mutex<crate::runner::Runner>) {
    rig.forge.set_down(false);
    let added = rig.ask(runner, "add", Some("7"));
    assert_eq!(added.get("error"), None, "{added}");
    wake(runner);
    rig.ask(runner, "drop", Some("7"));
    rig.forge.set_down(true);
}

#[test]
fn a_failed_board_read_waits_its_backoff_when_the_board_leaves_and_re_enters_the_rotation() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "drain", None);
    rig.forge.set_down(true);
    assert!(matches!(
        wake(&runner),
        Some(StepReport::BoardFailed { .. })
    ));
    take_and_free_the_slot(&rig, &runner);
    let reads = rig.forge.board_reads();

    rig.clock.advance(RETRY_FIRST - 1);
    wake(&runner);

    assert_eq!(rig.forge.board_reads(), reads, "read inside the backoff");
}

#[test]
fn the_board_is_read_again_once_its_backoff_has_passed() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "drain", None);
    rig.forge.set_down(true);
    wake(&runner);
    take_and_free_the_slot(&rig, &runner);
    let reads = rig.forge.board_reads();

    rig.clock.advance(RETRY_FIRST);
    wake(&runner);

    assert_eq!(rig.forge.board_reads(), reads + 1);
}

#[test]
fn no_board_step_runs_before_the_rate_limit_hold_ends() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    let before = rig.forge.board_reads();
    rig.forge
        .set_used_up(Some(Timestamp(rig.clock.now().0 + 1800)));
    wake(&runner);
    let reads = rig.forge.board_reads();
    assert_eq!(
        reads, before,
        "the board step ran past the hold the pass's first forge call hit"
    );

    for _ in 0..29 {
        rig.clock.advance(60);
        wake(&runner);
    }
    rig.clock.advance(59);
    wake(&runner);
    assert_eq!(rig.forge.board_reads(), reads, "read inside the hold");

    rig.forge.set_down(false);
    rig.clock.advance(1);
    wake(&runner);
    assert_eq!(rig.forge.board_reads(), reads + 1);
}

// Kelpie's comment on a pull request, through the ports the runner holds
fn comments(
    runner: &std::sync::Mutex<crate::runner::Runner>,
) -> Result<u64, crate::ports::ForgeError> {
    let runner = runner.lock().unwrap();
    runner.ports.forge.comment(&runner.remote, 71, "Merge it?")
}

// A write of kelpie's own changes what the forge shows, so the read that
// shows it is not left for the next poll.

#[test]
fn a_write_of_kelpies_own_makes_the_next_look_due_at_once() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    assert_eq!(step(&runner).unwrap(), None);
    rig.forge.set_down(false);
    let before = runner.lock().unwrap().next_look();
    assert_ne!(before, Some(Duration::from_secs(1)));

    let comment = comments(&runner);
    assert!(comment.is_ok(), "{comment:?}");

    assert_eq!(
        runner.lock().unwrap().next_look(),
        Some(Duration::from_secs(1))
    );
    step(&runner).unwrap();
    assert_eq!(
        runner.lock().unwrap().next_look(),
        before,
        "the pass after the write reads, so the wait is what it was"
    );
}

#[test]
fn a_write_that_failed_changes_nothing_to_look_at() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    assert_eq!(step(&runner).unwrap(), None);
    let before = runner.lock().unwrap().next_look();
    rig.forge.set_down(true);

    let comment = comments(&runner);
    assert!(comment.is_err());

    assert_eq!(runner.lock().unwrap().next_look(), before);
}
