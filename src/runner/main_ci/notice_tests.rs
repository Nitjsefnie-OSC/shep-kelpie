//! The rerun of a workflow that went red on `main`, and the notices to the
//! maintainer that follow it, through the runner's stand-ins

use std::sync::Mutex;

use crate::ports::{BranchRun, Clock, Timestamp};
use crate::runner::{MAIN_CI_POLL, Runner, StepReport, step};
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

// A runner that has read `test` fail in run 91 and acted on it
fn red_test() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("acme");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    (rig, runner)
}

// The next quarter hour's read, which finds `runs` on `main`, and what the
// passes after it did
fn read_again(rig: &Rig, runner: &Mutex<Runner>, runs: Vec<BranchRun>) -> Vec<StepReport> {
    rig.forge.set_branch_runs("main", runs);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    // The read ends a pass that did nothing else, and a notice goes in the next.
    step(runner).unwrap();
    let mut reports = Vec::new();
    while let Some(report) = step(runner).unwrap() {
        reports.push(report);
    }
    reports
}

fn log_lines(lines: std::ops::RangeInclusive<u32>) -> String {
    lines.map(|n| format!("line {n}\n")).collect()
}

#[test]
fn a_first_red_run_on_main_is_rerun_once() {
    let (rig, runner) = red_test();
    assert_eq!(rig.forge.run_reruns(), [91]);

    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);
    read_again(
        &rig,
        &runner,
        vec![run("test", 92, "failure", Rig::EPOCH + 1800)],
    );

    assert_eq!(rig.forge.run_reruns(), [91], "never rerun a second time");
}

#[test]
fn a_rerun_that_passes_posts_nothing() {
    let (rig, runner) = red_test();

    let reports = read_again(&rig, &runner, vec![run("test", 91, "success", Rig::EPOCH)]);

    assert_eq!(reports, []);
    assert!(rig.alerts.posts().is_empty());
}

#[test]
fn an_older_run_shown_while_the_rerun_is_going_neither_clears_nor_alerts() {
    let (rig, runner) = red_test();

    // Run 91 is not completed while its jobs rerun, so the runs before it show.
    let reports = read_again(
        &rig,
        &runner,
        vec![run("test", 80, "success", Rig::EPOCH - 3600)],
    );
    assert_eq!(reports, []);
    let reports = read_again(
        &rig,
        &runner,
        vec![run("test", 79, "failure", Rig::EPOCH - 7200)],
    );

    assert_eq!(reports, []);
    assert!(rig.alerts.posts().is_empty());
    assert_eq!(rig.forge.run_reruns(), [91]);
}

#[test]
fn a_red_rerun_posts_one_notice_with_the_log_tail() {
    let (rig, runner) = red_test();
    rig.forge.set_run_log_tail(91, &log_lines(1..=30));

    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);

    let [(webhook, alert)] = rig.alerts.posts().try_into().unwrap();
    assert_eq!(webhook, rig.webhook());
    assert_eq!(alert.title, "kelpie: acme test is red on main");
    let first_lines = "test has been red on main for 15m.\n\
        Run: https://github.com/shep-pm/shep/actions/runs/91\n\
        The end of its failed log:\n";
    assert_eq!(
        alert.text,
        format!("{first_lines}{}", log_lines(11..=30).trim_end())
    );
}

#[test]
fn a_long_log_is_cut_to_the_end_of_its_last_lines() {
    let (rig, runner) = red_test();
    rig.forge
        .set_run_log_tail(91, &format!("{}\n", "x".repeat(10_000)));

    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);

    let [(_, alert)] = rig.alerts.posts().try_into().unwrap();
    assert!(alert.text.len() < 4000, "{}", alert.text.len());
    assert!(alert.text.ends_with("xxx"), "{}", alert.text);
}

#[test]
fn a_second_red_head_posts_no_second_notice() {
    let (rig, runner) = red_test();
    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);
    assert_eq!(rig.alerts.posts().len(), 1);

    let reports = read_again(
        &rig,
        &runner,
        vec![run("test", 95, "failure", Rig::EPOCH + 3600)],
    );
    read_again(
        &rig,
        &runner,
        vec![run("test", 96, "timed_out", Rig::EPOCH + 7200)],
    );

    assert_eq!(reports, []);
    assert_eq!(rig.alerts.posts().len(), 1);
    assert_eq!(rig.forge.run_reruns(), [91]);
}

#[test]
fn a_later_head_that_is_red_while_the_rerun_goes_posts_the_notice() {
    let (rig, runner) = red_test();

    read_again(
        &rig,
        &runner,
        vec![run("test", 95, "failure", Rig::EPOCH + 600)],
    );

    let [(_, alert)] = rig.alerts.posts().try_into().unwrap();
    assert!(alert.text.contains("/actions/runs/95\n"), "{}", alert.text);
}

#[test]
fn green_after_a_notice_posts_a_green_notice() {
    let (rig, runner) = red_test();
    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);

    let reports = read_again(
        &rig,
        &runner,
        vec![run("test", 97, "success", Rig::EPOCH + 3 * 3600)],
    );

    assert_eq!(
        reports,
        [StepReport::MainNoticed {
            workflow: "test".into(),
            green: true
        }]
    );
    let [_, (webhook, alert)] = rig.alerts.posts().try_into().unwrap();
    assert_eq!(webhook, rig.webhook());
    assert_eq!(alert.title, "kelpie: acme test is green on main");
    assert_eq!(
        alert.text,
        "test is green on main again after 30m red.\n\
         Run: https://github.com/shep-pm/shep/actions/runs/97"
    );
    read_again(
        &rig,
        &runner,
        vec![run("test", 98, "success", Rig::EPOCH + 4 * 3600)],
    );
    assert_eq!(rig.alerts.posts().len(), 2, "green is told once");
}

#[test]
fn a_red_stretch_after_a_green_one_is_rerun_and_told_again() {
    let (rig, runner) = red_test();
    read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);
    read_again(
        &rig,
        &runner,
        vec![run("test", 97, "success", Rig::EPOCH + 3600)],
    );

    read_again(
        &rig,
        &runner,
        vec![run("test", 99, "failure", Rig::EPOCH + 7200)],
    );
    read_again(
        &rig,
        &runner,
        vec![run("test", 99, "failure", Rig::EPOCH + 7200)],
    );

    assert_eq!(rig.forge.run_reruns(), [91, 99]);
    assert_eq!(rig.alerts.posts().len(), 3);
}

#[test]
fn a_rerun_the_forge_refuses_posts_the_notice_at_once() {
    let rig = Rig::new("acme");
    rig.forge.set_rerun_error("gh: HTTP 403");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();

    step(&runner).unwrap();
    let reports = std::iter::from_fn(|| step(&runner).unwrap()).collect::<Vec<_>>();

    assert_eq!(rig.forge.run_reruns(), [] as [u64; 0]);
    assert_eq!(
        reports,
        [StepReport::MainNoticed {
            workflow: "test".into(),
            green: false
        }]
    );
}

#[test]
fn a_failed_post_is_tried_again_on_the_backoff() {
    let (rig, runner) = red_test();
    rig.alerts.set_down(true);
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    rig.clock.advance(MAIN_CI_POLL.as_secs());
    step(&runner).unwrap();
    let now = rig.clock.now().0;
    let failed = |wait| StepReport::MainNoticeFailed {
        workflow: "test".into(),
        green: false,
        reason: "the webhook answered HTTP 503".into(),
        retry_at: Timestamp(now + wait),
    };
    assert_eq!(step(&runner).unwrap(), Some(failed(60)));
    assert_eq!(step(&runner).unwrap(), None, "not before its retry");
    rig.clock.advance(60);
    assert_eq!(step(&runner).unwrap(), Some(failed(180)));

    rig.alerts.set_down(false);
    rig.clock.advance(120);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::MainNoticed {
            workflow: "test".into(),
            green: false
        })
    );
    assert_eq!(step(&runner).unwrap(), None);
}

#[test]
fn a_notice_with_no_webhook_is_logged_and_not_kept() {
    let rig = Rig::new("acme");
    rig.set_kelpie_settings("");
    rig.forge
        .set_branch_runs("main", vec![run("test", 91, "failure", Rig::EPOCH)]);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();

    let reports = read_again(&rig, &runner, vec![run("test", 91, "failure", Rig::EPOCH)]);

    assert_eq!(
        reports,
        [StepReport::MainNoticed {
            workflow: "test".into(),
            green: false
        }]
    );
    assert!(rig.alerts.posts().is_empty());
    assert_eq!(step(&runner).unwrap(), None, "and is not kept");
}

#[test]
fn a_red_run_older_than_the_one_rerun_is_not_the_rerun_failing() {
    // A workflow that was red before the rerun was ever made of its latest
    // run, as one from an older state file or after a limited first try is.
    let (rig, runner) = red_test();
    drop(runner);
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(rig.paths().state).unwrap()).unwrap();
    state["red"][0]["first_run"] = 80.into();
    state["red"][0]["since"] = (Rig::EPOCH - 3600).into();
    std::fs::write(rig.paths().state, state.to_string()).unwrap();
    let runner = rig.open().unwrap();

    // Run 91's jobs are rerunning, so the run before it shows in its place.
    let reports = read_again(
        &rig,
        &runner,
        vec![run("test", 90, "failure", Rig::EPOCH - 600)],
    );

    assert_eq!(reports, []);
    assert!(rig.alerts.posts().is_empty());
}
