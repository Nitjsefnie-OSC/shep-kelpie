//! The journal entries a fix turn's findings get, through the runner's
//! stand-ins

use std::sync::Mutex;

use crate::ports::{Finding, Severity};
use crate::runner::retro::journal::{self, Entry, FindingOutcome, Outcome};
use crate::runner::review_bot::thread_replies;
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted, ScriptedRound, ticket};

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
    std::fs::write(build.join("deferred-findings.md"), line_of(&c)).unwrap();
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
                outcome(&c, Outcome::Deferred, None),
            ]
        )]
    );
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
