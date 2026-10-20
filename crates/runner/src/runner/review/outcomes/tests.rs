//! The journal entries a fix turn's findings get, through the runner's
//! stand-ins

use std::sync::Mutex;

use crate::ports::{Finding, Severity, TestOutcome};
use crate::runner::retro::journal::{self, Entry, FindingOutcome, Outcome};
use crate::runner::review::findings::Sent;
use crate::runner::review_bot::thread_replies;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted, ScriptedRound, ticket};
use crate::work_item::{Phase, ReviewStage};

fn at(severity: Severity, file: &str, line: u32) -> Finding {
    Finding::new(severity, file, line, "what is wrong", "why")
}

// A MEDIUM finding as the worker copies it into its files.
fn line_of(f: &Finding) -> String {
    format!("MEDIUM|{}:{}|{}|{}\n", f.file, f.line, f.what, f.why)
}

// Round 1 found `found`, and the fix turn is next.
fn findings_sent(found: Vec<Finding>) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // opens the pull request, enters round 1 (qwen)
    rig.reviewer.script([ScriptedRound::Findings(found)]);
    step(&runner).unwrap(); // round 1's qwen call
    step(&runner).unwrap(); // the findings go out: the fix turn is next
    (rig, runner)
}

fn written(rig: &Rig) -> Vec<(u32, String, Vec<FindingOutcome>)> {
    journal::read(&rig.paths().journal)
        .into_iter()
        .filter_map(|s| match s.entry {
            Entry::FindingOutcomes {
                issue,
                round,
                reviewer,
                outcomes,
            } => {
                assert_eq!(issue, ticket(7));
                Some((round, reviewer, outcomes))
            }
            _ => None,
        })
        .collect()
}

fn outcome(f: &Finding, outcome: Outcome, reason: Option<&str>) -> FindingOutcome {
    FindingOutcome {
        severity: f.severity,
        file: f.file.clone(),
        line: f.line,
        outcome,
        reason: reason.map(str::to_owned),
    }
}

#[test]
fn a_fix_that_pushed_journals_pushed_disputed_and_deferred() {
    let (a, b, c) = (
        at(Severity::High, "src/a.rs", 1),
        at(Severity::Medium, "src/b.rs", 2),
        at(Severity::Medium, "src/c.rs", 3),
    );
    let (rig, runner) = findings_sent(vec![a.clone(), b.clone(), c.clone()]);
    let build = rig.build_7();
    std::fs::write(
        build.join("disputed-findings.md"),
        format!("{}  the closure takes two values\n", line_of(&b)),
    )
    .unwrap();
    std::fs::write(
        build.join("deferred-findings.md"),
        format!("{}  it needs its own pull request\n", line_of(&c)),
    )
    .unwrap();
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap(); // the fix turn pushes
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));

    assert_eq!(
        written(&rig),
        [(
            1,
            "qwen".to_owned(),
            vec![
                outcome(&a, Outcome::Pushed, None),
                outcome(&b, Outcome::Disputed, Some("the closure takes two values")),
                outcome(&c, Outcome::Deferred, Some("it needs its own pull request")),
            ]
        )]
    );
}

#[test]
fn a_fix_turn_that_disputed_one_finding_and_deferred_two_raises_the_counters() {
    let found: Vec<_> = (1..=4)
        .map(|n| at(Severity::Medium, &format!("src/{n}.rs"), n))
        .collect();
    let (rig, runner) = findings_sent(found.clone());
    let build = rig.build_7();
    std::fs::write(
        build.join("disputed-findings.md"),
        format!("{}  the closure takes two values\n", line_of(&found[1])),
    )
    .unwrap();
    std::fs::write(
        build.join("deferred-findings.md"),
        format!(
            "{}  it needs its own pull request\n{}  so does this\n",
            line_of(&found[2]),
            line_of(&found[3])
        ),
    )
    .unwrap();
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap(); // the fix turn pushes
    step(&runner).unwrap();

    let counts = runner.lock().unwrap().current().unwrap().counts;
    assert_eq!((counts.disputed, counts.deferred), (1, 2));
}

// A restart before the round leaves its fix stage runs the settled turn's
// path again, over the same files.
#[test]
fn a_fix_turn_that_settles_twice_counts_its_findings_once() {
    let (a, b) = (
        at(Severity::Medium, "src/a.rs", 1),
        at(Severity::Medium, "src/b.rs", 2),
    );
    let (rig, runner) = findings_sent(vec![a.clone(), b.clone()]);
    let (review, sent, deferred_before) = match runner.lock().unwrap().current().unwrap().phase {
        Phase::Review(ref review) => match review.stage {
            ReviewStage::Fixing {
                ref sent,
                ref deferred_before,
                ..
            } => (review.clone(), sent.clone(), deferred_before.clone()),
            ref stage => panic!("{stage:?}"),
        },
        ref phase => panic!("{phase:?}"),
    };
    std::fs::write(
        rig.build_7().join("disputed-findings.md"),
        format!("{}  the closure takes two values\n", line_of(&a)),
    )
    .unwrap();
    std::fs::write(
        rig.build_7().join("deferred-findings.md"),
        format!("{}  it needs its own pull request\n", line_of(&b)),
    )
    .unwrap();
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap(); // the fix turn pushes
    step(&runner).unwrap(); // it settles

    let sent = Sent {
        findings: &sent,
        deferred_before: &deferred_before,
    };
    runner
        .lock()
        .unwrap()
        .journal_outcomes(&review, sent, true)
        .unwrap();

    let counts = runner.lock().unwrap().current().unwrap().counts;
    assert_eq!((counts.disputed, counts.deferred), (1, 1));
}

// The second fix turn is sent the same finding again, with its worker files
// as they were, and the finding was counted by the first.
#[test]
fn a_dispute_is_counted_once_when_a_failing_test_sends_the_worker_back_to_the_round() {
    let rig = Rig::new("shep");
    rig.test_command("cargo test --test {name}");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // the worker's first turn
    step(&runner).unwrap(); // round 1, qwen: clean by default
    let failed = |output: &str| TestOutcome::Failed {
        output: output.to_owned(),
    };
    rig.tests
        .script([failed("before"), failed("after"), failed("again")]);
    let line = "MEDIUM|src/lib.rs:3|the flag is read first|a caller sees nothing|tests/flag.rs";
    rig.claude.script([Scripted::Attach(
        "tests/flag.rs",
        "#[test]\nfn flag_is_read() { assert!(false); }\n",
        line,
    )]);
    step(&runner).unwrap(); // round 2, claude
    step(&runner).unwrap(); // the findings sent
    std::fs::write(
        rig.build_7().join("disputed-findings.md"),
        format!("{line}\n  the test asserts the wrong thing\n"),
    )
    .unwrap();
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap(); // the worker's fix turn
    step(&runner).unwrap(); // it settles, and the test fails still
    let counted = |runner: &Mutex<Runner>| runner.lock().unwrap().current().unwrap().counts;
    assert_eq!(counted(&runner).disputed, 1);

    rig.claude.script([Scripted::Push("again.txt", "again\n")]);
    step(&runner).unwrap(); // the second fix turn
    step(&runner).unwrap(); // it settles on the same files

    assert_eq!(counted(&runner).disputed, 1);
}

#[test]
fn a_fix_that_pushed_nothing_and_was_sent_only_nits_journals_nit_declined() {
    let (a, b) = (
        at(Severity::Low, "src/a.rs", 1),
        at(Severity::Low, "src/b.rs", 2),
    );
    let (rig, runner) = findings_sent(vec![a.clone(), b.clone()]);
    rig.claude.script([Scripted::Say("They read fine.")]);
    step(&runner).unwrap(); // the fix turn ends with nothing pushed
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::NitsDeclined { nits: 2, .. })
    ));

    assert_eq!(
        written(&rig),
        [(
            1,
            "qwen".to_owned(),
            vec![
                outcome(&a, Outcome::NitDeclined, None),
                outcome(&b, Outcome::NitDeclined, None),
            ]
        )]
    );
}

#[test]
fn a_turn_that_raises_fix_not_pushed_journals_nothing_and_the_turn_after_it_does() {
    let a = at(Severity::High, "src/a.rs", 1);
    let (rig, runner) = findings_sent(vec![a.clone()]);
    rig.claude.script([Scripted::Say("I could not.")]);
    step(&runner).unwrap(); // the fix turn ends with nothing pushed
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Ruling { .. })
    ));
    assert_eq!(written(&rig), []);

    rig.ask(&runner, "rule", Some("71 yes"));
    rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
    step(&runner).unwrap(); // the worker's next turn pushes
    step(&runner).unwrap(); // and ends

    assert_eq!(
        written(&rig),
        [(
            1,
            "qwen".to_owned(),
            vec![outcome(&a, Outcome::Pushed, None)]
        )]
    );
}

#[test]
fn a_bots_threads_journal_what_the_worker_said_of_each_with_the_bot_as_reviewer() {
    let (rig, runner, _) = thread_replies::sent(&["P2: one", "P2: two", "P2: three"]);
    rig.claude.script([Scripted::PushSay(
        "strip.txt",
        "stripped\n",
        "thread 1: fixed\nthread 2: declined: it is meant",
    )]);
    step(&runner).unwrap(); // the worker's fix turn
    step(&runner).unwrap(); // the threads are answered

    let written = written(&rig);
    assert_eq!(written.len(), 1, "{written:?}");
    let (_, reviewer, outcomes) = &written[0];
    assert_eq!(reviewer, "cubic");
    let said: Vec<_> = (outcomes.iter())
        .map(|o| (o.outcome, o.reason.as_deref()))
        .collect();
    assert_eq!(
        said,
        [
            (Outcome::Fixed, None),
            (Outcome::Declined, Some("it is meant")),
            (Outcome::NotAddressed, None),
        ]
    );
}

#[test]
fn a_step_that_runs_again_after_the_forge_refused_a_thread_journals_one_entry() {
    let (rig, runner, _) = thread_replies::sent(&["P2: one", "P2: two"]);
    rig.forge.coderabbit.set_resolve_down(true);
    rig.claude.script([Scripted::PushSay(
        "strip.txt",
        "stripped\n",
        "thread 1: declined: it is meant\nthread 2: fixed",
    )]);
    step(&runner).unwrap(); // the worker's fix turn
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::GateFailed { .. })
    ));
    rig.forge.coderabbit.set_resolve_down(false);
    rig.next_look();
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::FixPushed { .. })
    ));

    let written = written(&rig);
    assert_eq!(written.len(), 1, "{written:?}");
    let said: Vec<_> = (written[0].2.iter())
        .map(|o| (o.outcome, o.reason.as_deref()))
        .collect();
    assert_eq!(
        said,
        [
            (Outcome::Declined, Some("it is meant")),
            (Outcome::Fixed, None)
        ]
    );
}
