//! The digest for a recorded set of work items

use serde_json::{Value, json};

use super::{Records, build, span};
use crate::ports::Timestamp;
use crate::runner::panel::Record;
use crate::runner::retro::journal::{Entry, Stamped};
use crate::state::Finished;
use crate::usage::Line;
use crate::work_item::Seconds;

fn finished(issue: u64, title: &str, pull_request: Option<u64>, at: u64, wall: u64) -> Finished {
    Finished {
        issue,
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
                issue: 11,
                checks: vec!["test".into()],
            },
        ),
        stamped(1500, Entry::Rebased { issue: 11 }),
        stamped(
            1550,
            Entry::Conflicted {
                issue: 11,
                files: vec!["docs/design-log.md".into()],
            },
        ),
        stamped(
            1600,
            Entry::Round {
                issue: 11,
                round: 1,
                reviewer: "claude".into(),
                findings: 3,
            },
        ),
        stamped(
            1601,
            Entry::FindingsSent {
                issue: 11,
                round: 1,
                held: 2,
            },
        ),
        stamped(
            1990,
            Entry::Friction {
                issue: 11,
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
        "- Review rounds reported: 1",
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
