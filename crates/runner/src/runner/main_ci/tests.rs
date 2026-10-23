//! The red workflows on `main`, through the runner's stand-ins

use serde_json::json;

use crate::ports::{BranchRun, Timestamp, Workflow, WorkflowState};
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
            "doing": "kelpie reran its failed jobs and waits for the result",
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
             https://github.com/shep-pm/shep/actions/runs/91 at head91; \
             kelpie reran its failed jobs and waits for the result\n"
        ),
        "{board}"
    );
}

#[test]
fn a_pull_request_runs_failure_does_not_mark_a_workflow_red_and_its_pass_does_not_clear_one() {
    let rig = Rig::new("acme");
    let pull_request = |id, conclusion, created| BranchRun {
        event: "pull_request".into(),
        ..run("test", id, conclusion, created)
    };
    rig.forge.set_branch_runs(
        "main",
        vec![
            pull_request(92, "failure", Rig::EPOCH + 60),
            run("test", 91, "success", Rig::EPOCH),
        ],
    );
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(status(&rig, &runner), json!(null));

    rig.forge.set_branch_runs(
        "main",
        vec![
            pull_request(94, "success", Rig::EPOCH + 1200),
            run("test", 93, "failure", Rig::EPOCH + 900),
        ],
    );
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    let red = status(&rig, &runner);
    assert_eq!(red[0]["run"], 93, "{red}");
}

#[test]
fn main_is_read_in_a_pass_where_a_work_item_acts() {
    let rig = Rig::new("acme");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    let before = rig.forge.branch_runs_reads();

    let report = step(&runner).unwrap();

    assert!(report.is_some(), "the work item acted");
    assert_eq!(rig.forge.branch_runs_reads(), before + 1);
}

fn workflow(name: &str, state: WorkflowState) -> Workflow {
    Workflow {
        name: name.into(),
        path: format!(".github/workflows/{name}.yml"),
        state,
    }
}

// A runner that has read `test` red, with the forge's workflows as `listed`
// says, and the quarter hour on so the next pass reads again
fn red_then_listed(rig: &Rig, listed: Vec<Workflow>) -> std::sync::Mutex<crate::runner::Runner> {
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(status(rig, &runner)[0]["workflow"], "test");
    rig.forge.set_workflows(listed);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    runner
}

#[test]
fn a_red_workflow_whose_file_was_deleted_drops_out() {
    let rig = Rig::new("acme");
    let runner = red_then_listed(&rig, vec![workflow("test", WorkflowState::Deleted)]);

    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), json!(null));
}

#[test]
fn a_red_workflow_the_forge_no_longer_lists_drops_out() {
    let rig = Rig::new("acme");
    let runner = red_then_listed(&rig, vec![workflow("lint", WorkflowState::Active)]);

    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), json!(null));
}

#[test]
fn a_disabled_red_workflow_drops_out() {
    for state in [
        WorkflowState::DisabledManually,
        WorkflowState::DisabledInactivity,
        WorkflowState::DisabledFork,
    ] {
        let rig = Rig::new("acme");
        let runner = red_then_listed(&rig, vec![workflow("test", state)]);

        step(&runner).unwrap();

        assert_eq!(status(&rig, &runner), json!(null), "{state:?}");
    }
}

#[test]
fn a_red_workflow_whose_latest_run_fell_outside_the_read_stays_red_while_it_is_active() {
    let rig = Rig::new("acme");
    let runner = red_then_listed(&rig, vec![workflow("test", WorkflowState::Active)]);
    let before = status(&rig, &runner);
    rig.forge
        .set_branch_runs("main", vec![run("lint", 99, "success", Rig::EPOCH + 900)]);

    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), before);
}

#[test]
fn a_workflow_that_fails_while_disabled_is_not_listed_as_red() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    rig.forge
        .set_workflows(vec![workflow("test", WorkflowState::DisabledManually)]);
    let runner = rig.open().unwrap();

    step(&runner).unwrap();

    assert_eq!(status(&rig, &runner), json!(null));
}

#[test]
fn the_workflows_are_listed_only_while_something_is_red() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "success", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(rig.forge.workflows_reads(), 0);

    rig.forge
        .set_branch_runs("main", vec![run("test", 93, "failure", Rig::EPOCH + 900)]);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    assert_eq!(rig.forge.workflows_reads(), 1);
    assert_eq!(status(&rig, &runner)[0]["workflow"], "test");
}

#[test]
fn a_workflow_list_that_fails_keeps_the_red_workflows_and_is_retried_on_the_backoff() {
    let rig = Rig::new("acme");
    let runner = red_then_listed(&rig, vec![workflow("test", WorkflowState::Deleted)]);
    let red = status(&rig, &runner);
    rig.forge.set_workflows_error("gh: HTTP 502");
    let before = rig.forge.workflows_reads();
    step(&runner).unwrap();
    assert_eq!(rig.forge.workflows_reads(), before + 1);

    rig.clock.advance(14);
    step(&runner).unwrap();
    assert_eq!(rig.forge.workflows_reads(), before + 1, "read 14s on");
    rig.clock.advance(1);
    step(&runner).unwrap();
    assert_eq!(rig.forge.workflows_reads(), before + 2, "no read 15s on");
    assert_eq!(status(&rig, &runner), red);
}

#[test]
fn a_head_rerun_under_a_state_file_from_before_reran_head_is_not_rerun_again() {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    assert_eq!(rig.forge.run_reruns(), [91]);
    drop(runner);
    let file = rig.paths().state;
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let red = &mut state["red"][0];
    assert_eq!(red["reran"], 91);
    red.as_object_mut().unwrap().remove("reran_head");
    std::fs::write(&file, serde_json::to_string(&state).unwrap()).unwrap();

    let runner = rig.open().unwrap();
    assert_eq!(
        status(&rig, &runner)[0]["doing"],
        "kelpie reran its failed jobs and waits for the result"
    );
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();

    assert_eq!(rig.forge.run_reruns(), [91], "no second rerun");
    assert_eq!(
        status(&rig, &runner)[0]["doing"],
        "red again after its rerun; the project manager opens a fix or asks"
    );
}
