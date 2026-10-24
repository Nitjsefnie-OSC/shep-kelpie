//! A step's read, run with the runner's lock let go and the step made again with its result
//!
//! Pull request 71 is the gate's: a pass reads it at the top of `check_ci`, which lists the open
//! pull requests first, so the held listing is the read the job makes.

use std::sync::atomic::Ordering;

use super::*;
use crate::ports::Timestamp;
use crate::ports::{Checks, Clock, ForgeError};
use crate::runner::CHECKS_SETTLE;
use crate::runner::trigger::lock;
use crate::runner::turn::Slot;
use crate::work_item::{Attached, Holder, Phase, Turn};

// A running project whose pull request 71 is red and has been for as long as the gate settles, so
// the next pass reads it and starts a fix turn
fn red(project: &str) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = Rig::with_pull_request(project);
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    assert!(matches!(advance(&runner), Ok(Pass::Idle)), "CI settles");
    rig.clock.advance(CHECKS_SETTLE);
    (rig, runner)
}

// How many calls the stand-in Claude has taken, the project's first turn among them
fn calls(rig: &Rig) -> usize {
    rig.claude.all_calls().len()
}

fn listed(rig: &Rig) -> usize {
    rig.forge.pull_request_reads().listed.load(Ordering::SeqCst)
}

fn viewed(rig: &Rig) -> usize {
    rig.forge.pull_request_reads().viewed.load(Ordering::SeqCst)
}

// Issue 7's work item as the state holds it
fn seven(runner: &Mutex<Runner>) -> Option<crate::work_item::WorkItem> {
    lock(runner).state.item(ticket(7)).cloned()
}

#[test]
fn a_trigger_is_answered_while_a_steps_read_is_held() {
    let (rig, runner, _) = Rig::with_pull_request("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (reply, pass) = rig.asked_while_held(&runner, read, "status", None);

    let reply = reply.expect("the trigger waited for the lock the read held");
    assert_eq!(reply["work_items"].as_array().unwrap().len(), 1);
    pass.unwrap();
}

#[test]
fn a_step_that_asks_for_a_read_is_stepped_again_with_the_result() {
    let (rig, runner) = red("koji");
    let (listings, views) = (listed(&rig), viewed(&rig));

    let pass = advance(&runner).unwrap();

    assert!(
        matches!(pass, Pass::Report(StepReport::CiFailed { .. })),
        "{pass:?}"
    );
    // The job's listing served the view: the step made again read nothing.
    assert_eq!(
        (listed(&rig) - listings, viewed(&rig) - views),
        (1, 0),
        "the step read again instead of taking the job's answer"
    );
    // The red head became the worker's next turn.
    let item = seven(&runner).expect("issue 7 is open");
    assert!(matches!(item.turn, Turn::Next { .. }), "{:?}", item.turn);
}

#[test]
fn a_read_whose_item_changed_phase_during_the_gap_is_dropped_and_the_slot_is_idle() {
    let (rig, runner) = red("koji");
    let (read, started) = (rig.forge.hold_next_open_pull_requests_read(), calls(&rig));
    let moved = Phase::Ci {
        head: Some("moved".into()),
        since: rig.clock.now(),
    };

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || {
            let mut held = lock(&runner);
            held.focus = Some(ticket(7));
            held.update(|item| item.phase = moved.clone()).unwrap();
        },
    );

    assert!(matches!(pass, Ok(Pass::Idle)), "{pass:?}");
    assert_eq!(calls(&rig), started);
    assert_eq!(seven(&runner).map(|item| item.phase), Some(moved));
}

#[test]
fn a_read_whose_item_was_dropped_during_the_gap_does_not_panic() {
    let (rig, runner) = red("koji");
    let (read, started) = (rig.forge.hold_next_open_pull_requests_read(), calls(&rig));

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || rig.ask(&runner, "drop", Some("7")),
    );

    assert!(matches!(pass, Ok(Pass::Idle)), "{pass:?}");
    assert_eq!(seven(&runner), None);
    assert_eq!(calls(&rig), started);
}

// `attach` changes nothing the `ItemKey` holds, so the step made again asks for itself
#[test]
fn an_item_attached_during_the_gap_is_not_stepped_again() {
    let (rig, runner) = red("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();
    let before = seven(&runner).expect("issue 7 is open");

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || {
            let mut held = lock(&runner);
            held.focus = Some(ticket(7));
            held.update(|item| {
                item.attached = Some(Attached {
                    by: Holder {
                        pid: 1,
                        started: "Mon Jan  1 00:00:00 2024".into(),
                    },
                    session: None,
                    since: Timestamp(Rig::EPOCH),
                });
            })
            .unwrap();
        },
    );

    assert!(matches!(pass, Ok(Pass::Idle)), "{pass:?}");
    let after = seven(&runner).expect("issue 7 is open");
    assert_eq!((after.phase, after.turn), (before.phase, before.turn));
}

#[test]
fn focus_is_set_again_after_a_gap_that_a_trigger_moved_it_in() {
    let (rig, runner) = red("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (_, pass) = rig.during_read(read, || advance(&runner), || lock(&runner).focus = None);

    // A step with its focus gone would find no work item to gate.
    let pass = pass.unwrap();
    assert!(
        matches!(&pass, Pass::Report(StepReport::CiFailed { issue, .. }) if *issue == ticket(7)),
        "{pass:?}"
    );
}

#[test]
fn a_state_change_during_a_gap_is_kept_by_the_step_that_follows() {
    let (rig, runner, _) = Rig::with_pull_request("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || {
            let mut held = lock(&runner);
            held.focus = Some(ticket(7));
            held.update(|item| item.rebased = true).unwrap();
        },
    );

    pass.unwrap();
    let item = seven(&runner).expect("issue 7 is open");
    assert!(
        item.rebased,
        "the step wrote back the work item it read before the gap"
    );
}

#[test]
fn a_failed_read_reaches_the_step_as_the_error_the_inline_call_returned() {
    let (rig, runner) = red("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || {
            // The listing fails, and so does the read of the pull request that follows it.
            rig.forge.set_board_down(true);
            rig.forge.set_down(true);
        },
    );

    let down = ForgeError::Failed("error connecting to api.github.com".into());
    match pass.unwrap() {
        Pass::Report(StepReport::GateFailed { issue, reason }) => {
            assert_eq!(issue, ticket(7));
            assert_eq!(reason, format!("cannot read #71: {down}"));
        }
        other => panic!("the failed read did not reach the gate: {other:?}"),
    }
}

// Issue 7's pull request waits on CI, so its step is idle after the read, and issue 8's turn is
// due in the same pass. The drain lands in 7's gap, before 8's turn would start.
#[test]
fn a_drain_that_lands_during_a_gap_starts_no_call() {
    let (rig, runner, _) = Rig::with_pull_request_set("koji", |rig| {
        rig.edit_settings(|s| s.replace("active_items = 1", "active_items = 2"));
    });
    rig.ask(&runner, "add", Some("8"));
    // The rotation goes on from the one after the last to act: 7 is stepped before 8.
    lock(&runner).last_acted = Some(Slot::Item(ticket(8)));
    let (read, started) = (rig.forge.hold_next_open_pull_requests_read(), calls(&rig));

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || rig.ask(&runner, "drain", None),
    );

    let pass = pass.unwrap();
    assert!(matches!(pass, Pass::Idle), "{pass:?}");
    assert_eq!(calls(&rig), started);
}

#[test]
fn snapshot_end_pass_runs_when_a_step_returns_early() {
    let (rig, runner) = red("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || rig.ask(&runner, "drop", Some("7")),
    );

    // The step ended with no work item to start a call, and the pass's snapshot ended with it.
    assert!(matches!(pass, Ok(Pass::Idle)), "{pass:?}");
    assert!(!lock(&runner).snapshot.in_pass());
}

// The runner begins a turn that died unborn again once the lock is dropped, through the rotation's
// own steps, so a gate item's read is made with the lock let go
#[test]
fn a_start_over_after_a_turn_ends_reads_with_the_lock_released() {
    let (rig, runner, _) = Rig::with_pull_request("koji");
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (reply, started) = rig.during_read(
        read,
        || rotation::start_over(&runner, ticket(7)),
        || rig.ask(&runner, "status", None),
    );

    let reply = reply.expect("the trigger waited for the lock the start-over's read held");
    assert_eq!(reply["work_items"].as_array().unwrap().len(), 1);
    assert_eq!(started.unwrap(), None);
    assert!(!lock(&runner).snapshot.in_pass());
}

// The gate's read of its pull request is made with the lock let go, so no hold `status` shows is
// half as long as the read's wait. A pass's own work holds the lock for a moment, so the bound is
// loose.
#[test]
fn a_pass_that_waits_on_a_held_gate_read_shows_no_hold_as_long_as_the_wait() {
    const HELD_FOR: Duration = Duration::from_secs(3);
    let (rig, runner, _) = Rig::with_pull_request("koji");
    let runner = Arc::new(runner);
    let read = rig.forge.hold_next_open_pull_requests_read();

    std::thread::scope(|scope| {
        let pass = scope.spawn(|| advance(&runner));
        assert!(
            read.waiting(CALL_PATIENCE),
            "the pass never reached the gate's read"
        );
        std::thread::sleep(HELD_FOR);
        drop(read);
        pass.join().expect("the pass thread").unwrap();
    });

    let status = rig.ask(&runner, "status", None);
    let longest = status["lock"]
        .as_object()
        .expect("the holds by site")
        .values()
        .filter_map(serde_json::Value::as_f64)
        .fold(0.0, f64::max);
    assert!(longest < HELD_FOR.as_secs_f64() / 2.0, "{longest}");
}

// Why a step that asked for a read is held up is said when the step ends. An item attached during
// the gap is not stepped again, so the reason must not wait for the next item's step to take it.
#[test]
fn a_reason_left_by_a_step_whose_item_was_attached_in_the_gap_is_not_given_to_the_next_item() {
    let rig = Rig::new("rotom");
    rig.ci_on();
    rig.push_by_hand("fix/timeline", "work.txt");
    let red = rig.push_by_hand("fix/timeline", "more.txt");
    rig.forge.open_pull_request(80, "fix/timeline", &[5]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "adopt", Some("80"));
    advance(&runner).unwrap();
    rig.forge
        .set_checks(&red, Checks::Failed(vec!["test".into()]));
    rig.verdict(&runner);
    rig.push_by_hand("fix/timeline", "owner.txt");
    rig.forge.set_lagging(80, Some(&red));
    let mut held = lock(&runner);
    let adopted = held.state.work_items[0].issue;
    let now = rig.clock.now();
    held.open_pass();

    let first = held.step_slot(Slot::Item(adopted), now).unwrap();
    assert!(
        matches!(first, Begin::Reads(_)),
        "the step asked for no read"
    );
    held.update(|item| {
        item.attached = Some(Attached {
            by: Holder {
                pid: 1,
                started: "Mon Jan  1 00:00:00 2024".into(),
            },
            session: None,
            since: Timestamp(Rig::EPOCH),
        });
    })
    .unwrap();
    let again = held.step_slot(Slot::Item(adopted), now).unwrap();
    assert!(matches!(again, Begin::Idle), "attached, yet stepped again");

    // The next item's step, idle for a reason of its own, finds nothing pending.
    let other = ticket(99);
    held.held_up.stepped(other, &Begin::Idle, now);
    assert_eq!(held.held_up.of(other), None);
}
