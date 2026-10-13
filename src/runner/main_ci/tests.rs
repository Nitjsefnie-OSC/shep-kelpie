//! The red workflows on `main`, through the runner's stand-ins

use serde_json::json;

use crate::ports::{BranchRun, Timestamp};
use crate::runner::{MAIN_CI_POLL, step};
use crate::test::Rig;

fn run(workflow: &str, id: u64, conclusion: &str, created: u64) -> BranchRun {
    BranchRun {
        workflow: workflow.into(),
        id,
        head: format!("head{id}"),
        event: "push".into(),
        conclusion: conclusion.into(),
        created: Timestamp(created),
    }
}

#[test]
fn a_failed_run_on_main_lists_its_workflow_as_red() {
    let rig = Rig::new("acme");
    let went_red = Rig::EPOCH - 3 * 3600 - 5 * 60;
    rig.forge.set_branch_runs(
        "main",
        vec![
            run("test", 91, "failure", went_red),
            run("lint", 92, "success", went_red),
        ],
    );
    let runner = rig.open().unwrap();

    step(&runner).unwrap();

    assert_eq!(
        rig.ask(&runner, "status", None)["red_workflows"],
        json!([{
            "workflow": "test",
            "first_run": 91,
            "run": 91,
            "head": "head91",
            "since": went_red,
            "red_for": 3 * 3600 + 5 * 60,
            "url": "https://github.com/shep-pm/shep/actions/runs/91",
        }])
    );
}

// The red workflows without how long each has been, which the clock moves
fn status(rig: &Rig, runner: &std::sync::Mutex<crate::runner::Runner>) -> serde_json::Value {
    let mut red = rig.ask(runner, "status", None)["red_workflows"].clone();
    for workflow in red.as_array_mut().into_iter().flatten() {
        workflow.as_object_mut().unwrap().remove("red_for");
    }
    red
}

#[test]
fn a_green_run_after_red_clears_the_workflow() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(status(&rig, &runner)[0]["workflow"], "test");

    rig.forge
        .set_branch_runs("main", vec![run("test", 93, "success", Rig::EPOCH + 600)]);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), json!(null));
}

#[test]
fn a_second_red_run_moves_the_latest_run_and_keeps_the_first() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();

    rig.forge.set_branch_runs(
        "main",
        vec![run("test", 95, "timed_out", Rig::EPOCH + 1800)],
    );
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    let red = status(&rig, &runner);
    assert_eq!(red.as_array().unwrap().len(), 1, "{red}");
    assert_eq!(red[0]["first_run"], 91);
    assert_eq!(red[0]["run"], 95);
    assert_eq!(red[0]["head"], "head95");
    assert_eq!(red[0]["since"], Rig::EPOCH);
}

#[test]
fn a_cancelled_run_leaves_the_workflow_as_it_was() {
    let rig = Rig::new("acme");
    rig.forge.set_branch_runs(
        "main",
        vec![
            run("test", 91, "failure", Rig::EPOCH),
            run("lint", 92, "success", Rig::EPOCH),
        ],
    );
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    let before = status(&rig, &runner);

    // A cancelled run of a red workflow changes nothing, and neither does a
    // skipped one of a workflow that was never red.
    rig.forge.set_branch_runs(
        "main",
        vec![
            run("test", 94, "cancelled", Rig::EPOCH + 900),
            run("lint", 95, "skipped", Rig::EPOCH + 900),
        ],
    );
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), before);
}

#[test]
fn main_is_read_at_most_once_in_fifteen_minutes() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    let before = rig.forge.branch_runs_reads();
    for _ in 0..30 {
        step(&runner).unwrap();
    }
    assert_eq!(rig.forge.branch_runs_reads(), before + 1);

    rig.clock.advance(MAIN_CI_POLL.as_secs() - 1);
    for _ in 0..30 {
        step(&runner).unwrap();
    }
    assert_eq!(
        rig.forge.branch_runs_reads(),
        before + 1,
        "read inside the quarter hour"
    );

    rig.clock.advance(1);
    for _ in 0..30 {
        step(&runner).unwrap();
    }
    assert_eq!(rig.forge.branch_runs_reads(), before + 2);
}

#[test]
fn a_read_that_fails_keeps_the_red_workflows_and_is_retried_on_the_backoff() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    let red = status(&rig, &runner);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    rig.forge.set_branch_runs_error("gh: HTTP 502");
    let before = rig.forge.branch_runs_reads();
    step(&runner).unwrap();
    assert_eq!(rig.forge.branch_runs_reads(), before + 1);

    let mut reads = before + 1;
    for wait in [15, 30] {
        rig.clock.advance(wait - 1);
        step(&runner).unwrap();
        assert_eq!(rig.forge.branch_runs_reads(), reads, "read {wait}s on");
        rig.clock.advance(1);
        step(&runner).unwrap();
        reads += 1;
        assert_eq!(rig.forge.branch_runs_reads(), reads, "no read {wait}s on");
    }
    assert_eq!(status(&rig, &runner), red);
}

#[test]
fn red_workflows_survive_a_restart() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    let red = status(&rig, &runner);
    drop(runner);

    let reopened = rig.open().unwrap();

    assert_eq!(status(&rig, &reopened), red);
}

#[test]
fn plain_status_names_the_red_workflow_how_long_it_has_been_red_and_its_run() {
    let rig = Rig::new("acme");
    let went_red = Rig::EPOCH - 3 * 3600 - 5 * 60;
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", went_red)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    let body = rig.ask(&runner, "status", None).to_string();

    let lines = crate::flock::control::plain_status("acme", &body);

    assert!(lines.contains(&"  red on main: 1".to_owned()), "{lines:#?}");
    assert!(
        lines.contains(
            &"    test: red for 3h05m, https://github.com/shep-pm/shep/actions/runs/91".to_owned()
        ),
        "{lines:#?}"
    );
}

#[test]
fn the_board_names_the_red_workflow_how_long_it_has_been_red_and_its_run() {
    let rig = Rig::new("acme");
    let went_red = Rig::EPOCH - 3 * 3600 - 5 * 60;
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", went_red)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();

    crate::runner::briefing::brief(&runner);
    let board = std::fs::read_to_string(rig.paths().board).unwrap();

    assert!(
        board.contains(
            "## Red on main\n\n- test: red for 3h05m, latest run \
             https://github.com/shep-pm/shep/actions/runs/91 at head91\n"
        ),
        "{board}"
    );
}
