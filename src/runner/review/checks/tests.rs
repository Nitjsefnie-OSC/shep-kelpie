use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use crate::ports::TestOutcome::{self, Failed, NotRun, Passed, TimedOut};
use crate::ports::{Finding, Severity};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};
use crate::work_item::{Reading, TestCheck};

const TEST_FILE: &str = "tests/flag.rs";
const TEST_TEXT: &str = "#[test]\nfn flag_is_read() { assert!(false); }\n";
const FINDING: &str =
    "MEDIUM|src/lib.rs:3|the flag is read first|a caller sees nothing|tests/flag.rs";

fn failed(output: &str) -> TestOutcome {
    Failed {
        output: output.to_owned(),
    }
}

// Round 2, the claude reviewer's, is due, and the project has a test command.
fn asking_tests() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.test_command("cargo test --test {name}");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // the worker's first turn
    step(&runner).unwrap(); // round 1, qwen: clean by default
    (rig, runner)
}

// The claude reviewer attaches `file` to its finding, and the findings are
// sent: the run before the fix is made in this step.
fn sent(rig: &Rig, runner: &Mutex<Runner>, file: &'static str, reply: &'static str) {
    rig.claude
        .script([Scripted::Attach(file, TEST_TEXT, reply)]);
    step(runner).unwrap(); // round 2, claude
    step(runner).unwrap(); // the findings sent
}

// The worker's fix turn pushes, and the fix ends: the run after is made.
fn fixed(rig: &Rig, runner: &Mutex<Runner>) -> StepReport {
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(runner).unwrap(); // the worker's fix turn
    step(runner).unwrap().expect("the fix ended with a report")
}

fn checks_of(runner: &Mutex<Runner>) -> Vec<TestCheck> {
    runner.lock().unwrap().state.work_items[0]
        .test_checks
        .clone()
}

fn phase(rig: &Rig, runner: &Mutex<Runner>) -> serde_json::Value {
    rig.ask(runner, "status", None)["work_item"]["phase"]["state"].clone()
}

fn the_flag_check(before: TestOutcome, after: Option<TestOutcome>) -> TestCheck {
    TestCheck {
        file: "src/lib.rs".into(),
        line: 3,
        what: "the flag is read first".into(),
        round: 2,
        before,
        after,
        sent_back: false,
    }
}

#[test]
fn a_test_that_fails_before_and_passes_after_is_recorded_verified() {
    let (rig, runner) = asking_tests();
    rig.tests.script([failed("assertion failed: flag"), Passed]);

    sent(&rig, &runner, TEST_FILE, FINDING);
    assert_eq!(
        checks_of(&runner),
        [the_flag_check(failed("assertion failed: flag"), None)],
        "the run before the fix is recorded as soon as it is made"
    );
    let report = fixed(&rig, &runner);

    let verified = the_flag_check(failed("assertion failed: flag"), Some(Passed));
    assert_eq!(verified.reading(), Reading::Verified);
    assert_eq!(checks_of(&runner), vec![verified.clone()]);
    let head = rig.forge.head_of("kelpie/7");
    assert_eq!(
        report,
        StepReport::FixPushed {
            issue: 7,
            pull_request: 71,
            round: 2,
            head,
            checks: vec![verified],
            unchecked: 0,
        }
    );
    let item = &rig.ask(&runner, "status", None)["work_item"];
    assert_eq!(
        item["test_checks"],
        json!([{
            "file": "src/lib.rs",
            "line": 3,
            "what": "the flag is read first",
            "round": 2,
            "before": { "result": "failed", "output": "assertion failed: flag" },
            "after": { "result": "passed" },
        }]),
        "status shows both outcomes"
    );
}

#[test]
fn a_test_that_passes_before_the_fix_is_not_reproducing_and_blocks_nothing() {
    let (rig, runner) = asking_tests();
    rig.tests.script([Passed, Passed]);

    sent(&rig, &runner, TEST_FILE, FINDING);
    fixed(&rig, &runner);

    let checks = checks_of(&runner);
    assert_eq!(checks, [the_flag_check(Passed, Some(Passed))]);
    assert_eq!(checks[0].reading(), Reading::NotReproducing);
    assert_eq!(phase(&rig, &runner), "ci", "the pass went on");
    let held = runner.lock().unwrap().state.work_items[0].held.clone();
    assert_eq!(held.len(), 1, "the finding is still held");
}

// The worker's turn after a send-back pushes, and the turn ends.
fn fixed_again(rig: &Rig, runner: &Mutex<Runner>) -> StepReport {
    rig.claude.script([Scripted::Push("again.txt", "again\n")]);
    step(runner).unwrap(); // the worker's second fix turn
    step(runner).unwrap().expect("the fix ended with a report")
}

fn sent_back(check: TestCheck) -> TestCheck {
    TestCheck {
        sent_back: true,
        ..check
    }
}

fn rulings(rig: &Rig, runner: &Mutex<Runner>) -> serde_json::Value {
    rig.ask(runner, "status", None)["rulings"].clone()
}

#[test]
fn a_test_still_failing_after_the_fix_sends_the_worker_back_once() {
    let (rig, runner) = asking_tests();
    rig.tests.script([failed("before"), failed("after")]);
    sent(&rig, &runner, TEST_FILE, FINDING);

    let report = fixed(&rig, &runner);

    assert!(matches!(report, StepReport::FixPushed { .. }), "{report:?}");
    assert_eq!(
        phase(&rig, &runner),
        "review",
        "the pass waits on the worker"
    );
    assert_eq!(rulings(&rig, &runner), json!([]));
    assert_eq!(
        checks_of(&runner),
        [sent_back(the_flag_check(
            failed("before"),
            Some(failed("after"))
        ))],
        "the failed run stays, with the mark"
    );
    rig.claude.script([Scripted::Push("again.txt", "again\n")]);
    step(&runner).unwrap(); // the worker's second fix turn
    let prompt = rig.claude.calls().last().unwrap().prompt.clone();
    let kept = rig.paths().worker.join("review-tests-7");
    assert!(prompt.contains(TEST_FILE), "{prompt}");
    assert!(prompt.contains("after"), "{prompt}");
    assert!(!prompt.contains("before"), "{prompt}");
    assert!(prompt.contains("leave the test alone"), "{prompt}");
    assert!(
        !prompt.contains(kept.to_str().unwrap()),
        "the worker reads the build folder's copy: {prompt}"
    );
    assert_eq!(rig.tests.ran().len(), 2, "the test is not run mid-turn");
}

#[test]
fn a_test_still_failing_after_the_second_fix_raises_a_stuck_ruling_naming_it() {
    let (rig, runner) = asking_tests();
    rig.tests
        .script([failed("before"), failed("after"), failed("after again")]);
    sent(&rig, &runner, TEST_FILE, FINDING);
    fixed(&rig, &runner);

    let report = fixed_again(&rig, &runner);

    let StepReport::Ruling {
        id: 71, question, ..
    } = report
    else {
        panic!("{report:?}");
    };
    assert!(question.contains(TEST_FILE), "{question}");
    assert!(question.contains("after again"), "{question}");
    assert_eq!(phase(&rig, &runner), "ruling");
    let head = rig.forge.head_of("kelpie/7").unwrap();
    let kind = rulings(&rig, &runner)[0]["kind"].clone();
    assert_eq!(kind["reason"], "test-still-fails", "{kind}");
    assert_eq!(kind["head"], head.as_str(), "{kind}");
    assert_eq!(kind["review"]["round"], 2, "{kind}");
    assert_eq!(
        kind["tests"],
        json!([{ "test": TEST_FILE, "output": "after again" }])
    );
    assert_eq!(rig.tests.ran().len(), 3);
}

#[test]
fn a_test_that_passes_on_the_second_fix_is_verified_and_raises_nothing() {
    let (rig, runner) = asking_tests();
    rig.tests
        .script([failed("before"), failed("after"), Passed]);
    sent(&rig, &runner, TEST_FILE, FINDING);
    fixed(&rig, &runner);

    let report = fixed_again(&rig, &runner);

    assert!(matches!(report, StepReport::FixPushed { .. }), "{report:?}");
    let verified = sent_back(the_flag_check(failed("before"), Some(Passed)));
    assert_eq!(checks_of(&runner), [verified]);
    assert_eq!(rulings(&rig, &runner), json!([]));
    assert_eq!(phase(&rig, &runner), "ci", "the pass went on");
}

#[test]
fn a_fix_that_pushes_nothing_raises_fix_not_pushed_not_this_ruling() {
    let (rig, runner) = asking_tests();
    rig.tests.script([failed("before"), failed("after")]);
    sent(&rig, &runner, TEST_FILE, FINDING);
    fixed(&rig, &runner);

    rig.claude.script([Scripted::Text("Nothing to push.")]);
    step(&runner).unwrap(); // the worker's second fix turn
    let report = step(&runner).unwrap().expect("the fix ended with a report");

    assert!(
        matches!(report, StepReport::Ruling { id: 71, .. }),
        "{report:?}"
    );
    let kind = rulings(&rig, &runner)[0]["kind"].clone();
    assert_eq!(kind["reason"], "fix-not-pushed", "{kind}");
    assert_eq!(
        rig.tests.ran().len(),
        2,
        "no run for a fix that moved nothing"
    );
}

#[test]
fn a_run_that_timed_out_or_never_ran_does_not_send_the_worker_back() {
    for after in [
        TimedOut,
        NotRun {
            reason: "the command could not start".into(),
        },
    ] {
        let (rig, runner) = asking_tests();
        rig.tests.script([failed("before"), after]);
        sent(&rig, &runner, TEST_FILE, FINDING);
        fixed(&rig, &runner);
        assert_eq!(phase(&rig, &runner), "ci", "the pass went on");
        assert_eq!(rulings(&rig, &runner), json!([]));
        assert!(!checks_of(&runner)[0].sent_back);
    }
}

#[test]
fn the_test_file_is_gone_from_the_worktree_after_each_run() {
    let (rig, runner) = asking_tests();
    rig.tests.watch(TEST_FILE);
    rig.tests.script([failed("no"), Passed]);

    sent(&rig, &runner, TEST_FILE, FINDING);
    let worktree = rig.worktree_7();
    assert!(
        !worktree.join(TEST_FILE).exists(),
        "gone after the first run"
    );
    assert!(!worktree.join("tests").exists(), "and the folder it made");
    fixed(&rig, &runner);
    assert!(
        !worktree.join(TEST_FILE).exists(),
        "gone after the second run"
    );
    assert!(!worktree.join("tests").exists());

    let ran = rig.tests.ran();
    assert_eq!(ran.len(), 2);
    for run in &ran {
        assert_eq!(
            run.held,
            [PathBuf::from(TEST_FILE)],
            "it was there as the run ran"
        );
    }
    // The reviewer's copy for the next run is untouched.
    let kept = rig.paths().worker.join("review-tests-7").join(TEST_FILE);
    assert!(kept.is_file());
}

#[test]
fn a_run_past_its_ceiling_is_timed_out_and_a_command_that_cannot_start_is_not_run() {
    let (rig, runner) = asking_tests();
    let cannot = NotRun {
        reason: "the command could not start: sh: cargo: not found".into(),
    };
    rig.tests.script([TimedOut, cannot.clone()]);

    sent(&rig, &runner, TEST_FILE, FINDING);
    fixed(&rig, &runner);

    let checks = checks_of(&runner);
    assert_eq!(checks, [the_flag_check(TimedOut, Some(cannot))]);
    assert_eq!(checks[0].reading(), Reading::Unknown);
    assert_eq!(phase(&rig, &runner), "ci", "neither blocks the pass");
    for run in rig.tests.ran() {
        assert_eq!(run.run.ceiling, Duration::from_secs(10 * 60));
    }
}

#[test]
fn the_after_run_waits_for_the_workers_own_head() {
    let (rig, runner) = asking_tests();
    rig.tests.script([failed("no"), Passed]);
    sent(&rig, &runner, TEST_FILE, FINDING);

    // Someone else's push moves the branch, and the worker pushes nothing.
    let by_hand = rig.push_by_hand("kelpie/7", "by-hand.txt");
    rig.claude.script([Scripted::Text("Looked at it.")]);
    step(&runner).unwrap(); // the worker's fix turn
    let report = step(&runner).unwrap().unwrap();

    let before_only = the_flag_check(failed("no"), None);
    assert_eq!(
        report,
        StepReport::FixPushed {
            issue: 7,
            pull_request: 71,
            round: 2,
            head: Some(by_hand),
            checks: vec![before_only.clone()],
            unchecked: 1,
        },
        "the report says the test was not run after the fix"
    );
    assert_eq!(checks_of(&runner), [before_only]);
    assert_eq!(rig.tests.ran().len(), 1, "only the run before the fix");
    assert_eq!(phase(&rig, &runner), "ci", "the pass went on");
}

#[test]
fn the_command_gets_the_tests_path_and_name_filled_in() {
    let rig = Rig::new("shep");
    rig.test_command("cargo test --test {name} -- {test}");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // the worker's first turn
    step(&runner).unwrap(); // round 1, qwen: clean by default

    sent(&rig, &runner, TEST_FILE, FINDING);

    let ran = rig.tests.ran();
    assert_eq!(ran.len(), 1);
    let run = &ran[0].run;
    assert_eq!(run.command, "cargo test --test flag -- tests/flag.rs");
    assert_eq!(run.cwd, rig.worktree_7());
    assert!(
        run.reach.fence.is_some(),
        "it runs under the worker's fence"
    );
    assert_eq!(run.door, rig.paths().door);
    assert!(run.settings.starts_with(rig.paths().worker));
}

#[test]
fn a_path_a_shell_would_read_is_recorded_not_run() {
    let (rig, runner) = asking_tests();

    sent(
        &rig,
        &runner,
        "tests/a;b.rs",
        "MEDIUM|src/lib.rs:3|the flag is read first|a caller sees nothing|tests/a;b.rs",
    );

    let checks = checks_of(&runner);
    assert_eq!(checks.len(), 1);
    let NotRun { reason } = &checks[0].before else {
        panic!("{:?}", checks[0]);
    };
    assert!(reason.contains("tests/a;b.rs"), "{reason}");
    assert!(rig.tests.ran().is_empty(), "no command was built from it");
}

#[test]
fn a_finding_without_a_test_is_never_run() {
    let (rig, runner) = asking_tests();
    rig.claude.script([Scripted::Text(
        "MEDIUM|src/lib.rs:3|the flag is read first|a caller sees nothing",
    )]);
    step(&runner).unwrap(); // round 2, claude
    step(&runner).unwrap(); // the findings sent
    fixed(&rig, &runner);

    assert!(rig.tests.ran().is_empty());
    assert!(checks_of(&runner).is_empty());
    let status = rig.ask(&runner, "status", None);
    assert!(status["work_item"].get("test_checks").is_none());
}

fn finding_testing(path: &str) -> Finding {
    Finding {
        severity: Severity::Medium,
        file: "src/lib.rs".into(),
        line: 3,
        what: "the flag is read first".into(),
        why: "a caller sees nothing".into(),
        test: Some(path.to_owned()),
    }
}

// The path comes from the state file, which is read as it stands.
#[test]
fn a_path_that_leaves_the_worktree_is_recorded_not_run() {
    let (rig, runner) = asking_tests();
    let guard = runner.lock().unwrap();
    let head = guard.worktree_head().unwrap();
    let found = [
        finding_testing("../escape.rs"),
        finding_testing("/etc/escape.rs"),
    ];

    let checks = guard.tests_before(2, &head, &found);

    assert_eq!(checks.len(), 2);
    for check in &checks {
        let NotRun { reason } = &check.before else {
            panic!("{check:?}");
        };
        assert!(reason.contains("inside the worktree"), "{reason}");
    }
    assert!(rig.tests.ran().is_empty(), "no command was built from them");
}

#[test]
fn files_a_run_leaves_in_a_folder_the_test_made_are_reported() {
    let (rig, runner) = asking_tests();
    rig.tests.leave("tests/left.txt");
    rig.tests.script([failed("no"), Passed]);

    sent(&rig, &runner, TEST_FILE, FINDING);

    let worktree = rig.worktree_7();
    assert!(!worktree.join(TEST_FILE).exists(), "the test itself is out");
    assert!(
        worktree.join("tests/left.txt").exists(),
        "the run's own file stays"
    );
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(notes.contains("left the worktree dirty"), "{notes}");
    assert!(notes.contains("tests"), "{notes}");
    assert_eq!(
        checks_of(&runner),
        [the_flag_check(failed("no"), None)],
        "the outcome is recorded all the same"
    );
}

// The fix prompt sends the worker to read the reviewer's test, and the natural
// fix commits a copy of it at its path.
#[test]
fn a_test_the_worker_committed_unchanged_is_still_run_after_the_fix_and_left() {
    let (rig, runner) = asking_tests();
    rig.tests.watch(TEST_FILE);
    rig.tests.script([failed("no"), Passed]);
    sent(&rig, &runner, TEST_FILE, FINDING);

    rig.claude.script([Scripted::Push(TEST_FILE, TEST_TEXT)]);
    step(&runner).unwrap(); // the worker's fix turn
    step(&runner).unwrap().expect("the fix ended with a report");

    let verified = the_flag_check(failed("no"), Some(Passed));
    assert_eq!(verified.reading(), Reading::Verified);
    assert_eq!(checks_of(&runner), [verified]);
    let ran = rig.tests.ran();
    assert_eq!(ran.len(), 2);
    assert_eq!(ran[1].held, [PathBuf::from(TEST_FILE)]);
    let worktree = rig.worktree_7();
    assert_eq!(
        std::fs::read_to_string(worktree.join(TEST_FILE)).unwrap(),
        TEST_TEXT,
        "the worker's committed file is the worker's, and stays"
    );
    assert!(!rig.paths().worker.join("test-placed-7").exists());
}

#[test]
fn a_file_of_that_name_with_other_text_is_not_run_over_or_removed() {
    let (rig, runner) = asking_tests();
    rig.tests.script([failed("no"), Passed]);
    sent(&rig, &runner, TEST_FILE, FINDING);

    let other = "#[test]\nfn flag_is_read() {}\n";
    rig.claude.script([Scripted::Push(TEST_FILE, other)]);
    step(&runner).unwrap(); // the worker's fix turn
    step(&runner).unwrap().expect("the fix ended with a report");

    let checks = checks_of(&runner);
    let Some(NotRun { reason }) = &checks[0].after else {
        panic!("{checks:?}");
    };
    assert!(reason.contains("another file"), "{reason}");
    assert_eq!(rig.tests.ran().len(), 1, "only the run before the fix");
    let worktree = rig.worktree_7();
    assert_eq!(
        std::fs::read_to_string(worktree.join(TEST_FILE)).unwrap(),
        other
    );
}

// A kelpie stopped mid-run leaves the file, and the record of it.
#[test]
fn a_test_left_in_the_worktree_by_a_stopped_run_is_taken_out_and_reported() {
    let (rig, runner) = asking_tests();
    rig.claude
        .script([Scripted::Attach(TEST_FILE, TEST_TEXT, FINDING)]);
    step(&runner).unwrap(); // round 2, claude
    let worktree = rig.worktree_7();
    std::fs::create_dir(worktree.join("tests")).unwrap();
    std::fs::write(worktree.join(TEST_FILE), TEST_TEXT).unwrap();
    let record = rig.paths().worker.join("test-placed-7");
    std::fs::write(&record, "tests/flag.rs\ntests\n").unwrap();
    rig.tests.watch(TEST_FILE);
    rig.tests.script([failed("no")]);

    step(&runner).unwrap(); // the findings sent

    assert!(!worktree.join("tests").exists(), "the file and its folder");
    assert!(!record.exists());
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(notes.contains("was stopped during a test run"), "{notes}");
    assert!(!notes.contains("dirty"), "{notes}");
    assert_eq!(checks_of(&runner), [the_flag_check(failed("no"), None)]);
    assert_eq!(
        rig.tests.ran().len(),
        1,
        "and the run was made all the same"
    );
}

#[test]
fn a_run_records_the_test_it_puts_in_the_worktree_until_it_is_taken_out() {
    let (rig, runner) = asking_tests();
    let record = rig.paths().worker.join("test-placed-7");
    rig.tests.watch(record.to_str().unwrap());
    rig.tests.script([failed("no")]);

    sent(&rig, &runner, TEST_FILE, FINDING);

    assert_eq!(
        rig.tests.ran()[0].held,
        vec![record.clone()],
        "the record stood as the test ran"
    );
    assert!(!record.exists(), "gone once the test is out");
}
