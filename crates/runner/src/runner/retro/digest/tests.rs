//! The digest for a recorded set of work items

use serde_json::{Value, json};

use super::{Records, build, span};
use crate::ports::{Severity, Timestamp};
use crate::runner::panel::Record;
use crate::runner::retro::journal::{Entry, Held, Stamped};
use crate::state::Finished;
use crate::test::ticket;
use crate::usage::Line;
use crate::work_item::Seconds;

fn finished(issue: u64, title: &str, pull_request: Option<u64>, at: u64, wall: u64) -> Finished {
    Finished {
        issue: ticket(issue),
        title: title.to_owned(),
        pull_request,
        merged: pull_request.is_some(),
        closed: pull_request.is_none(),
        at: Timestamp(at),
        wall,
        seconds: Seconds::default(),
        spend: None,
    }
}

fn call(at: u64, issue: Option<u64>, role: &str, kind: &str, units: u64, cost: f64) -> Line {
    line(json!({
        "line": "call", "at": at, "issue": issue, "role": role, "kind": kind,
        "usage": {"input": 0, "cache_write": 0, "cache_read": 0, "output": 0},
        "units": units, "cost_usd": cost, "ended": "answered",
    }))
}

fn line(value: Value) -> Line {
    serde_json::from_value(value).unwrap()
}

fn role(calls: u32, units: u64, cost: f64) -> Value {
    json!({
        "calls": calls, "units": units, "cost_usd": cost,
        "tokens": {"input": 0, "cache_write": 0, "cache_read": 0, "output": 0},
    })
}

fn ruling(value: Value) -> Record {
    serde_json::from_value(value).unwrap()
}

fn stamped(at: u64, entry: Entry) -> Stamped {
    Stamped {
        at: Timestamp(at),
        entry,
    }
}

fn wake(for_: &str, acted: &[&str]) -> Entry {
    Entry::Wake {
        woke_for: vec![for_.to_owned()],
        acted: acted.iter().map(|a| (*a).to_owned()).collect(),
        dropped: Vec::new(),
    }
}

// Two work items finished after a retro that began at 1000: #11 merged
// with a pull request, #12 closed with no change.
fn digest() -> String {
    let batch = [
        finished(11, "fix(retro): the digest", Some(71), 2000, 3725),
        finished(12, "Already done upstream", None, 3000, 42),
    ];
    let journal = [
        stamped(500, wake("#11 CI failed", &[])),
        stamped(1200, wake("#11 CI failed", &["marked #11 ready"])),
        stamped(1300, wake("#12 labels", &[])),
        stamped(1400, wake("#11 conflict, #110 too", &[])),
        stamped(
            1450,
            Entry::CiFailed {
                issue: ticket(11),
                checks: vec!["test".into()],
            },
        ),
        stamped(1500, Entry::Rebased { issue: ticket(11) }),
        stamped(
            1550,
            Entry::Conflicted {
                issue: ticket(11),
                files: vec!["docs/design-log.md".into()],
            },
        ),
        stamped(
            1600,
            Entry::Round {
                issue: ticket(11),
                round: 1,
                reviewer: "claude".into(),
                findings: 3,
            },
        ),
        stamped(
            1601,
            Entry::FindingsSent {
                issue: ticket(11),
                round: 1,
                held: 2,
            },
        ),
        stamped(
            1990,
            Entry::Friction {
                issue: ticket(11),
                text: "the lockfile\nfought me".into(),
            },
        ),
    ];
    let rulings = [
        ruling(json!({
            "id": 1, "at": 1900, "raised": 1600, "issue": 11, "pull_request": 71,
            "asked": "Merge?", "answer": "yes", "kind": "merge",
            "ballots": [
                {"voter": "worker", "vote": {"choice": "A", "why": "ok", "design": false}},
                {"voter": "advisor", "vote": null},
            ],
        })),
        ruling(json!({
            "id": 2, "at": 2900, "raised": 2810, "issue": 12, "pull_request": null,
            "asked": "Close?", "answer": "yes", "by_pm": true, "kind": "merge",
        })),
        ruling(json!({
            "id": 3, "at": 2950, "issue": 99, "pull_request": null,
            "asked": "Other?", "answer": "yes", "kind": "merge",
        })),
    ];
    let ledger = [
        call(1100, Some(11), "worker", "turn", 100, 1.50),
        call(1700, Some(11), "worker", "turn", 50, 0.50),
        call(1800, Some(11), "reviewer", "review", 30, 0.25),
        call(1250, None, "pm", "wake", 20, 0.10),
        call(500, None, "pm", "wake", 7, 0.07),
        call(2500, None, "pm", "retro", 400, 4.00),
        call(1900, Some(99), "worker", "turn", 9, 0.09),
        line(json!({
            "line": "finished", "at": 2000, "issue": 11, "title": "fix(retro): the digest",
            "pull_request": 71, "merged": true, "wall": 3725, "units": 180, "cost_usd": 2.25,
            "worker": role(2, 150, 2.0), "reviewer": role(1, 30, 0.25),
            "qwen": {"rounds": 0, "seconds": 0},
            "review_rounds": 1, "fix_turns": 1, "rulings": 1,
        })),
    ];
    build(&Records {
        project: "koji",
        batch: &batch,
        journal: &journal,
        rulings: &rulings,
        ledger: &ledger,
        since: 1000,
        gaps: &[],
    })
}

fn section<'a>(digest: &'a str, heading: &str) -> &'a str {
    let from = digest
        .find(heading)
        .unwrap_or_else(|| panic!("{heading}: {digest}"));
    let rest = &digest[from..];
    let to = rest[heading.len()..]
        .find("\n## ")
        .map_or(rest.len(), |n| n + heading.len());
    &rest[..to]
}

#[test]
fn the_header_names_the_project_and_the_batchs_issues() {
    let digest = digest();
    assert_eq!(
        digest.lines().next().unwrap(),
        "# Batch digest: koji, 2 finished work items (#11, #12)"
    );
}

#[test]
fn an_item_carries_its_title_type_and_ledger_line() {
    let digest = digest();
    let item = section(&digest, "## #11:");
    assert!(
        item.starts_with("## #11: fix(retro): the digest (fix)"),
        "{item}"
    );
    assert!(
        item.contains("- Ended: merged, pull request #71; wall time 1h02m"),
        "{item}"
    );
    assert!(
        item.contains(
            "- Ledger: 180 units, $2.25; worker 2 calls $2.00; reviewer 1 calls $0.25; \
             1 review rounds, 1 fix turns, 1 rulings"
        ),
        "{item}"
    );
}

#[test]
fn an_item_carries_the_wakes_that_named_it_and_what_they_did() {
    let digest = digest();
    let item = section(&digest, "## #11:");
    assert!(
        item.contains("- Project manager wakes that named it: 3"),
        "{item}"
    );
    assert!(
        item.contains("  - woke for #11 CI failed: changed nothing"),
        "{item}"
    );
    assert!(
        item.contains("  - woke for #11 CI failed: marked #11 ready"),
        "{item}"
    );
    // "#110" is another issue
    assert!(
        item.contains("  - woke for #11 conflict, #110 too: changed nothing"),
        "{item}"
    );
    let other = section(&digest, "## #12:");
    assert!(
        other.contains("- Project manager wakes that named it: 1"),
        "{other}"
    );
}

#[test]
fn an_item_carries_each_ruling_with_its_votes_and_who_answered() {
    let digest = digest();
    let item = section(&digest, "## #11:");
    assert!(item.contains("- Rulings: 1"), "{item}");
    assert!(
        item.contains(
            "  - ruling 1 (merge), waited 5m00s; votes: Worker A, Advisor abstained; \
             answered by the maintainer: yes"
        ),
        "{item}"
    );
    let other = section(&digest, "## #12:");
    assert!(
        other.contains(
            "  - ruling 2 (merge), waited 1m30s; no panel vote; \
             answered by the project manager: yes"
        ),
        "{other}"
    );
}

#[test]
fn an_item_carries_its_review_rounds_ci_fixes_merges_and_friction() {
    let digest = digest();
    let item = section(&digest, "## #11:");
    for expected in [
        "- Review rounds reported: 1, the latest numbered 1",
        "  - #11 round 1 by claude: 3 findings, 2 sent to the worker",
        "- Fix turns after a failed check: 1",
        "  - failed: test",
        "- Merges of main: 1 rebased, 1 conflicted",
        "  - conflict in docs/design-log.md",
        "- Friction the worker reported: the lockfile fought me",
    ] {
        assert!(item.contains(expected), "{expected}: {item}");
    }
}

#[test]
fn an_item_closed_with_no_change_says_so_and_has_no_ledger_line() {
    let digest = digest();
    let item = section(&digest, "## #12:");
    assert!(
        item.contains("- Ended: closed with no change; wall time 42s"),
        "{item}"
    );
    assert!(!item.contains("- Ledger:"), "{item}");
}

#[test]
fn the_totals_match_the_sums_by_hand() {
    let digest = digest();
    let totals = section(&digest, "## Batch totals");
    // Worker turns 100 + 50 units and $1.50 + $0.50, the reviewer's 30 and
    // $0.25, the project manager's one wake in the window (20, $0.10). Left
    // out: the wake before the retro, the retro's own call and #99's turn.
    // Wakes in the window: 1200, 1300 and 1400, two of which changed nothing.
    // Rulings 1 and 2 waited 300s and 90s.
    let expected = "## Batch totals\n\
        - Spend by role and kind:\n  \
        - pm wake: 1 calls, 20 units, $0.10\n  \
        - reviewer review: 1 calls, 30 units, $0.25\n  \
        - worker turn: 2 calls, 150 units, $2.00\n\
        - Project manager wakes: 3, of which 2 changed nothing\n\
        - Rulings: 2, waiting 6m30s in all and 5m00s at the longest\n";
    assert_eq!(totals, expected);
}

#[test]
fn a_span_reads_in_the_largest_units() {
    assert_eq!(span(42), "42s");
    assert_eq!(span(310), "5m10s");
    assert_eq!(span(3900), "1h05m");
}

fn conflicted(issue: u64, files: &[&str]) -> Entry {
    Entry::Conflicted {
        issue: ticket(issue),
        files: files.iter().map(|f| (*f).to_owned()).collect(),
    }
}

fn outside(issue: u64, tests: &[&str]) -> Entry {
    Entry::CiOutsideDiff {
        issue: ticket(issue),
        tests: tests.iter().map(|t| (*t).to_owned()).collect(),
    }
}

fn round(issue: u64, round: u32, findings: usize) -> Entry {
    Entry::Round {
        issue: ticket(issue),
        round,
        reviewer: "claude".into(),
        findings,
    }
}

fn held(severity: Severity, file: &str, line: u32, what: &str) -> Held {
    Held {
        severity,
        file: file.to_owned(),
        line,
        what: what.to_owned(),
        again: false,
    }
}

// Three merged work items, all finished after the last retro
fn across(journal: &[Stamped], gaps: &[String]) -> String {
    let batch = [
        finished(11, "fix(a): one", Some(71), 2000, 60),
        finished(12, "fix(b): two", Some(72), 2100, 60),
        finished(13, "fix(c): three", Some(73), 2200, 60),
    ];
    build(&Records {
        project: "koji",
        batch: &batch,
        journal,
        rulings: &[],
        ledger: &[],
        since: 1000,
        gaps,
    })
}

#[test]
fn the_conflicts_per_file_match_the_hand_count_of_a_recorded_batch() {
    // By hand: design-log.md conflicted four times, on #11, #12 and #13;
    // src/settings.rs once, on #11, and src/state.rs once, on #13. #99 is
    // not in the batch.
    let journal = [
        stamped(
            1100,
            conflicted(11, &["docs/design-log.md", "src/settings.rs"]),
        ),
        stamped(1200, conflicted(12, &["docs/design-log.md"])),
        stamped(
            1300,
            conflicted(13, &["docs/design-log.md", "src/state.rs"]),
        ),
        stamped(1400, conflicted(13, &["docs/design-log.md"])),
        stamped(1500, conflicted(99, &["src/settings.rs", "src/state.rs"])),
    ];

    let digest = across(&journal, &[]);

    let table = section(&digest, "## Across the batch");
    let expected = "- Merge conflicts per file, most first:\n  \
        - docs/design-log.md: 4 conflicts on #11, #12, #13\n  \
        - src/settings.rs: 1 conflicts on #11\n  \
        - src/state.rs: 1 conflicts on #13\n";
    assert!(table.contains(expected), "{table}");
}

#[test]
fn a_batch_with_no_conflict_says_so() {
    let digest = across(&[], &[]);
    let table = section(&digest, "## Across the batch");
    assert!(
        table.contains("- Merge conflicts per file: none\n"),
        "{table}"
    );
}

#[test]
fn rounds_per_item_and_each_findings_round_are_given() {
    let journal = [
        stamped(1100, round(11, 1, 2)),
        stamped(
            1101,
            Entry::Findings {
                issue: ticket(11),
                round: 1,
                findings: vec![
                    held(Severity::Low, "src/a.rs", 4, "a nit"),
                    held(Severity::Medium, "src/b.rs", 0, "wrong wording"),
                ],
            },
        ),
        stamped(1200, round(11, 2, 0)),
        stamped(1300, round(13, 1, 1)),
        stamped(1400, round(13, 2, 1)),
        stamped(1500, round(13, 3, 1)),
        stamped(
            1501,
            Entry::Findings {
                issue: ticket(13),
                round: 3,
                findings: vec![held(Severity::High, "src/gate.rs", 88, "the design leaks")],
            },
        ),
    ];

    let digest = across(&journal, &[]);

    let table = section(&digest, "## Across the batch");
    let expected = "- Review rounds per work item:\n  - #11: 2\n  - #12: 0\n  - #13: 3\n";
    assert!(table.contains(expected), "{expected}: {table}");
    let item = section(&digest, "## #11:");
    for expected in [
        "- Review rounds reported: 2, the latest numbered 2",
        "- Findings sent to the worker:\n  \
         - round 1 by claude, low, src/a.rs:4: a nit\n  \
         - round 1 by claude, medium, src/b.rs: wrong wording\n",
    ] {
        assert!(item.contains(expected), "{expected}: {item}");
    }
    let late = section(&digest, "## #13:");
    assert!(
        late.contains("  - round 3 (late) by claude, high, src/gate.rs:88: the design leaks"),
        "{late}"
    );
}

#[test]
fn a_finding_a_later_round_raised_again_is_marked() {
    let journal = [
        stamped(1100, round(11, 1, 1)),
        stamped(
            1101,
            Entry::Findings {
                issue: ticket(11),
                round: 1,
                findings: vec![held(Severity::High, "src/a.rs", 4, "the lock leaks")],
            },
        ),
        stamped(1200, round(11, 2, 2)),
        stamped(
            1201,
            Entry::Findings {
                issue: ticket(11),
                round: 2,
                findings: vec![
                    held(Severity::High, "src/a.rs", 4, "the lock leaks").again(true),
                    held(Severity::Medium, "src/b.rs", 9, "a new defect"),
                ],
            },
        ),
    ];

    let digest = across(&journal, &[]);

    let item = section(&digest, "## #11:");
    let expected = "- Findings sent to the worker:\n  \
        - round 1 by claude, high, src/a.rs:4: the lock leaks\n  \
        - round 2 (again) by claude, high, src/a.rs:4: the lock leaks\n  \
        - round 2 by claude, medium, src/b.rs:9: a new defect\n";
    assert!(item.contains(expected), "{expected}: {item}");
}

#[test]
fn a_test_failing_outside_two_items_diffs_is_a_known_flake_and_one_item_is_not() {
    let journal = [
        stamped(
            1100,
            outside(11, &["sheep::tests::reconnects", "only::here"]),
        ),
        stamped(1200, outside(12, &["sheep::tests::reconnects"])),
        stamped(1300, outside(12, &["sheep::tests::reconnects"])),
        stamped(1400, outside(99, &["only::here"])),
    ];

    let digest = across(&journal, &[]);

    let table = section(&digest, "## Across the batch");
    assert!(
        table.contains("- Known flakes:\n  - sheep::tests::reconnects: #11, #12\n"),
        "{table}"
    );
    assert!(!table.contains("only::here"), "{table}");
}

#[test]
fn a_known_gap_is_counted_by_the_items_that_name_it_and_not_described_again() {
    let gaps = vec![
        "sandbox cannot bind a socket".to_owned(),
        "stale lockfile".to_owned(),
    ];
    let journal = [
        stamped(
            1100,
            Entry::Friction {
                issue: ticket(11),
                text: "The Sandbox cannot bind a socket, so tests only ran in CI".into(),
            },
        ),
        stamped(1200, wake("#12 sandbox cannot bind a socket", &[])),
        stamped(1300, wake("#13 something else", &[])),
    ];

    let digest = across(&journal, &gaps);

    let table = section(&digest, "## Across the batch");
    assert!(
        table.contains(
            "- Known gaps named again in this batch:\n  \
             - sandbox cannot bind a socket: #11, #12\n"
        ),
        "{table}"
    );
    assert!(!table.contains("stale lockfile"), "{table}");
}
