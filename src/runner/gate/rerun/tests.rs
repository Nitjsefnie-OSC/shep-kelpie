use serde_json::json;

use super::super::CHECKS_SETTLE;
use super::super::tests::ruling_report;
use crate::ports::Checks;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

// The worker's first turn changed `work.txt`, so that is the branch's one changed file.
const OTHER_FILE_PANIC: &str = "--- run 9\nthread 'sheep::tests::reconnects' panicked at \
                                src/sheep.rs:412:9:\nassertion failed: elapsed < limit";

fn red(rig: &Rig, runner: &std::sync::Mutex<Runner>, head: &str, log: &str) -> StepReport {
    rig.forge.set_ci_log(log);
    rig.forge
        .set_checks(head, Checks::Failed(vec!["test".to_owned()]));
    rig.verdict(runner).expect("the gate read the red run")
}

fn fix_prompt(rig: &Rig, runner: &std::sync::Mutex<Runner>) -> String {
    rig.claude.script([Scripted::Say("fixed")]);
    step(runner).unwrap();
    rig.claude.calls().last().unwrap().prompt.clone()
}

#[test]
fn a_red_run_whose_log_names_no_changed_file_is_rerun_once_and_sends_no_turn() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    let calls = rig.claude.calls().len();

    let report = red(&rig, &runner, &head, OTHER_FILE_PANIC);

    assert_eq!(
        report,
        StepReport::CiRerun {
            issue: 7,
            pull_request: 71,
            head: head.clone(),
            checks: vec!["test".to_owned()],
        }
    );
    assert_eq!(rig.forge.reruns(), std::slice::from_ref(&head));
    assert_eq!(rig.claude.calls().len(), calls, "no fix turn went out");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["rerunning"], json!(head));

    // The rollup still reads red for a moment after the rerun starts.
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(CHECKS_SETTLE);
    rig.forge.set_checks(&head, Checks::Passed);
    let (_, question) = ruling_report(step(&runner).unwrap());
    assert!(question.starts_with("Merge pull request #71"), "{question}");
    assert_eq!(rig.forge.reruns().len(), 1);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"].get("rerunning"), None);
}

#[test]
fn a_head_red_again_after_its_rerun_sends_the_fix_turn_saying_it_repeated() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    red(&rig, &runner, &head, OTHER_FILE_PANIC);
    rig.clock.advance(CHECKS_SETTLE);

    let report = step(&runner).unwrap();

    assert!(
        matches!(report, Some(StepReport::CiFailed { head: ref h, .. }) if *h == head),
        "{report:?}"
    );
    assert_eq!(rig.forge.reruns().len(), 1, "a head is rerun once");
    let prompt = fix_prompt(&rig, &runner);
    assert!(
        prompt.contains(
            "kelpie reran this head's failed jobs and they failed again: the failure repeats"
        ),
        "{prompt}"
    );
}

#[test]
fn a_red_run_whose_log_names_a_changed_file_sends_the_fix_turn() {
    let (rig, runner, head) = Rig::with_pull_request("koji");

    let log = "--- run 9\nerror[E0308]: mismatched types\n --> work.txt:3:9";
    let report = red(&rig, &runner, &head, log);

    assert!(matches!(report, StepReport::CiFailed { .. }), "{report:?}");
    assert_eq!(rig.forge.reruns(), Vec::<String>::new());
    let prompt = fix_prompt(&rig, &runner);
    assert!(!prompt.contains("failed again"), "{prompt}");
}

#[test]
fn an_unreadable_log_sends_the_fix_turn() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    rig.forge.set_ci_log_error("gh run list failed");
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".to_owned()]));
    let report = rig.verdict(&runner).expect("the gate read the red run");
    assert!(matches!(report, StepReport::CiFailed { .. }), "{report:?}");
    assert_eq!(rig.forge.reruns(), Vec::<String>::new());

    // A log with nothing in it is as unreadable.
    let (rig, runner, head) = Rig::with_pull_request("rotom");
    let report = red(&rig, &runner, &head, "");
    assert!(matches!(report, StepReport::CiFailed { .. }), "{report:?}");
    assert_eq!(rig.forge.reruns(), Vec::<String>::new());
}

#[test]
fn a_rerun_the_forge_refuses_sends_the_fix_turn() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    rig.forge.set_rerun_error("gh failed: no admin rights");

    let report = red(&rig, &runner, &head, OTHER_FILE_PANIC);

    assert!(matches!(report, StepReport::CiFailed { .. }), "{report:?}");
    assert_eq!(rig.forge.reruns(), Vec::<String>::new());
}

#[test]
fn a_head_red_while_the_review_waits_for_it_is_rerun_and_the_review_waits_on_the_rerun() {
    let rig = Rig::new("shep");
    rig.ci_on();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();
    let head = rig.forge.head_of("kelpie/7").unwrap();
    let calls = rig.claude.all_seen().len();

    let report = red(&rig, &runner, &head, OTHER_FILE_PANIC);

    assert!(matches!(report, StepReport::CiRerun { .. }), "{report:?}");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["phase"]["state"], "review");
    assert_eq!(status["work_item"]["rerunning"], json!(head));
    assert_eq!(step(&runner).unwrap(), None, "the rerun is waited out");
    assert_eq!(rig.claude.all_seen().len(), calls, "no turn, no read");

    rig.forge.set_checks(&head, Checks::Passed);
    rig.clock.advance(CHECKS_SETTLE);
    assert!(step(&runner).unwrap().is_some(), "the review goes on");
    assert_eq!(rig.forge.reruns().len(), 1);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["work_item"].get("rerunning"),
        None,
        "a rerun that passed is no longer running"
    );
}
