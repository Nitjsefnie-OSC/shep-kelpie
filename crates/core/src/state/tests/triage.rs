use std::fs;

use super::super::*;
use super::store_in;
use crate::ports::Timestamp;

// A version 59 file has no `triage`, so it loads with no `since`; once set it
// is kept, and it is left out while unset.
#[test]
fn a_file_saved_before_this_version_loads_with_no_since() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 59.into();
    old.as_object_mut().unwrap().remove("triage");
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.triage, None);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert!(saved.get("triage").is_none(), "left out while unset");

    loaded.triage = Some(TriageState {
        since: Timestamp(1_791_964_800),
        issues: Vec::new(),
        unrecorded: Vec::new(),
        pruned_through: 0,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["triage"],
        serde_json::json!({ "since": 1_791_964_800 })
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 60 file has a `since` and no `issues`, so it loads with no records;
// records are kept with their step, and left out while there are none.
#[test]
fn a_file_saved_before_this_version_loads_with_no_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 60.into();
    old["triage"] = serde_json::json!({ "since": 1_791_964_800 });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_mut().expect("the since");
    assert_eq!(triage.since, Timestamp(1_791_964_800));
    assert_eq!(triage.issues, []);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"],
        serde_json::json!({ "since": 1_791_964_800 })
    );

    let triage = loaded.triage.as_mut().unwrap();
    triage.issues.push(Triaged {
        issue: 12,
        comment: 5001,
        step: TriageStep::Labelled,
        at: Timestamp(0),
        author: String::new(),
        held: None,
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: None,
        local: None,
        spam: None,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([{ "issue": 12, "comment": 5001, "at": 0, "step": "labelled" }])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 62 file has a record with no `duplicate_of`, so it loads with
// none. A closed record keeps the item it was closed as a duplicate of, and
// the step that says the close is still owed.
#[test]
fn a_file_saved_before_this_version_loads_with_no_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 62.into();
    old["triage"] = serde_json::json!({
        "since": 1_791_964_800,
        "issues": [{ "issue": 12, "comment": 5001, "step": "done" }],
    });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_mut().expect("the since");
    assert_eq!(
        triage.issues,
        [Triaged {
            issue: 12,
            comment: 5001,
            step: TriageStep::Done,
            at: Timestamp(0),
            author: String::new(),
            held: None,
            calls: Vec::new(),
            labels: Vec::new(),
            duplicate_of: None,
            ready_body: None,
            seen: Vec::new(),
            asked: None,
            questions: None,
            local: None,
            spam: None,
        }]
    );

    triage.issues.push(Triaged {
        issue: 13,
        comment: 5002,
        step: TriageStep::Closed,
        at: Timestamp(0),
        author: String::new(),
        held: None,
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: Some(41),
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: None,
        local: None,
        spam: None,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"][1],
        serde_json::json!({
            "issue": 13, "comment": 5002, "at": 0, "step": "closed", "duplicate_of": 41
        })
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// Past `TRIAGED_KEPT` finished records the oldest go, and a record still short
// of `done` stays whatever its age.
#[test]
fn the_oldest_finished_records_go_past_the_cap() {
    let record = |issue: u64, step: TriageStep| Triaged {
        issue,
        comment: issue + 1_000,
        step,
        at: Timestamp(0),
        author: String::new(),
        held: None,
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: None,
        local: None,
        spam: None,
    };
    let mut triage = TriageState {
        since: Timestamp(1),
        issues: vec![record(1, TriageStep::Commented)],
        unrecorded: Vec::new(),
        pruned_through: 0,
    };
    let cap = TRIAGED_KEPT as u64;
    triage
        .issues
        .extend((2..cap + 5).map(|n| record(n, TriageStep::Done)));

    triage.prune();

    assert_eq!(triage.issues.len(), TRIAGED_KEPT + 1);
    assert_eq!(triage.issues[0].issue, 1, "an unfinished record stays");
    assert_eq!(triage.issues[1].issue, 5, "the oldest finished ones went");
    assert_eq!(triage.issues.last().map(|t| t.issue), Some(cap + 4));
    assert_eq!(
        triage.pruned_through, 4,
        "the highest issue number a prune dropped is kept"
    );
}

// The daily caps count the calls the records hold, so a record that holds one is
// kept ahead of one that holds none: a burst of issues a limit held cannot push
// out the records the caps count, which is what they hold the burst for.
#[test]
fn a_prune_past_the_cap_lets_the_records_that_hold_no_call_go_first() {
    let cap = TRIAGED_KEPT as u64;
    let mut triage = TriageState {
        since: Timestamp(1),
        issues: (1..cap + 4)
            .map(|issue| Triaged {
                issue,
                comment: 0,
                step: TriageStep::Done,
                at: Timestamp(0),
                author: "newcomer".to_owned(),
                held: Some(TriageHeld::AuthorCap),
                calls: if issue <= 2 {
                    vec![Timestamp(9)]
                } else {
                    Vec::new()
                },
                labels: Vec::new(),
                duplicate_of: None,
                ready_body: None,
                seen: Vec::new(),
                asked: None,
                questions: None,
                local: None,
                spam: None,
            })
            .collect(),
        unrecorded: Vec::new(),
        pruned_through: 0,
    };

    triage.prune();

    assert_eq!(triage.issues.len(), TRIAGED_KEPT);
    assert_eq!(
        (triage.issues.iter())
            .take(2)
            .map(|t| t.issue)
            .collect::<Vec<_>>(),
        [1, 2],
        "the two oldest records hold calls, and they stayed"
    );
    assert_eq!(
        triage.issues[2].issue, 6,
        "the oldest records that hold no call went instead"
    );
    assert_eq!(triage.issues.last().map(|t| t.issue), Some(cap + 3));
}

// A record that closed an issue as a duplicate is kept past the cap: the issue
// carries no `needs-triage` label, and the record is what keeps a reopened one
// out of the due list.
#[test]
fn a_record_that_closed_a_duplicate_is_kept_past_the_cap() {
    let cap = TRIAGED_KEPT as u64;
    let mut triage = TriageState {
        since: Timestamp(1),
        issues: (1..cap + 3)
            .map(|issue| Triaged {
                issue,
                comment: issue + 1_000,
                step: TriageStep::Done,
                at: Timestamp(0),
                author: String::new(),
                held: None,
                calls: Vec::new(),
                labels: Vec::new(),
                duplicate_of: match issue {
                    2 => Some(41),
                    _ => None,
                },
                ready_body: None,
                seen: Vec::new(),
                asked: None,
                questions: None,
                local: None,
                spam: None,
            })
            .collect(),
        unrecorded: Vec::new(),
        pruned_through: 0,
    };

    triage.prune();

    assert_eq!(triage.issues.len(), TRIAGED_KEPT + 1);
    assert_eq!(
        triage.issues.first().map(|t| t.issue),
        Some(2),
        "the oldest ordinary record went and the duplicate's stayed"
    );
    assert_eq!(triage.issues[1].duplicate_of, None);
}

// A version 63 file, saved by the first pass before the ready path, has
// records with none of the new steps, no `ready_body` and no `seen`, so they
// load as they were and save again at the current version.
#[test]
fn a_record_saved_by_slice_2_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 63.into();
    old["triage"] = serde_json::json!({
        "since": 1_791_964_800,
        "issues": [
            { "issue": 12, "comment": 5001, "step": "commented", "labels": ["bug", "P2"] },
            { "issue": 13, "comment": 5002, "step": "labelled" },
            { "issue": 14, "comment": 5003, "step": "done" },
            { "issue": 15, "comment": 5004, "step": "closed", "duplicate_of": 41 },
        ],
    });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_ref().expect("the records");
    assert_eq!(triage.issues.len(), 4);
    assert!(
        triage
            .issues
            .iter()
            .all(|t| t.ready_body.is_none() && t.seen.is_empty()),
        "{:?}",
        triage.issues
    );
    assert_eq!(triage.issues[0].labels, ["bug", "P2"]);
    assert_eq!(triage.issues[3].duplicate_of, Some(41));

    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([
            { "issue": 12, "comment": 5001, "at": 0, "step": "commented", "labels": ["bug", "P2"] },
            { "issue": 13, "comment": 5002, "at": 0, "step": "labelled" },
            { "issue": 14, "comment": 5003, "at": 0, "step": "done" },
            { "issue": 15, "comment": 5004, "at": 0, "step": "closed", "duplicate_of": 41 },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 64 file, saved before the author's round, has records with no
// `asked`, no `questions` and no `answers` step, so they load with no ask and
// save again at the current version. A waiting record keeps the ask and the
// questions with the time each went up, and both are left out while unset.
#[test]
fn a_file_saved_before_this_version_loads_with_no_ask() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 64.into();
    old["triage"] = serde_json::json!({
        "since": 1_791_964_800,
        "issues": [{ "issue": 12, "comment": 5001, "step": "commented", "labels": ["bug"] }],
    });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_ref().expect("the records");
    assert_eq!(triage.issues[0].asked, None);
    assert_eq!(triage.issues[0].questions, None);

    let triage = loaded.triage.as_mut().unwrap();
    triage.issues[0].asked = Some(TriageAsked {
        at: Timestamp(1_792_051_200),
        asks: 2,
    });
    triage.issues.push(Triaged {
        issue: 13,
        comment: 5002,
        step: TriageStep::Answers,
        at: Timestamp(0),
        author: String::new(),
        held: None,
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: Some(TriageQuestions {
            comment: 5010,
            at: Timestamp(1_792_054_800),
        }),
        local: None,
        spam: None,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([
            {
                "issue": 12, "comment": 5001, "at": 0, "step": "commented", "labels": ["bug"],
                "asked": { "at": 1_792_051_200, "asks": 2 },
            },
            {
                "issue": 13, "comment": 5002, "at": 0, "step": "answers",
                "questions": { "comment": 5010, "at": 1_792_054_800 },
            },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 67 file, saved before the local round, has records with no `local`
// wait, so they load with none and save again at the current version. A waiting
// record keeps the time its prompt went up and the hash of the body it was
// posted against, and the wait is left out while unset.
#[test]
fn a_file_saved_before_this_version_loads_with_no_local_wait() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 67.into();
    old["triage"] = serde_json::json!({
        "since": 1_791_964_800,
        "issues": [
            { "issue": 12, "comment": 5001, "step": "commented", "labels": ["bug"] },
            { "issue": 13, "comment": 5002, "step": "done" },
        ],
    });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_ref().expect("the records");
    assert!(
        triage.issues.iter().all(|t| t.local.is_none()),
        "{:?}",
        triage.issues
    );

    let triage = loaded.triage.as_mut().unwrap();
    triage.issues[0].local = Some(TriageLocal {
        at: Timestamp(1_792_051_200),
        body: 18_446_744_073_709_551_615,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([
            {
                "issue": 12, "comment": 5001, "at": 0, "step": "commented", "labels": ["bug"],
                "local": { "at": 1_792_051_200, "body": 18_446_744_073_709_551_615_u64 },
            },
            { "issue": 13, "comment": 5002, "at": 0, "step": "done" },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 68 file has records with no `at`, no `author`, no `held` and no
// `calls`, so they load with none of them: no time, so no cap counts them, and
// no author, so no author's count reaches them. A record a limit held keeps the
// limit it was held by, with no comment id and no call behind it.
#[test]
fn a_file_saved_before_this_version_loads_with_no_limit_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 68.into();
    old["triage"] = serde_json::json!({
        "since": 1_791_964_800,
        "issues": [{ "issue": 12, "comment": 5001, "step": "done" }],
    });
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    let triage = loaded.triage.as_mut().expect("the records");
    assert_eq!(triage.issues[0].at, Timestamp(0), "no time to count");
    assert_eq!(triage.issues[0].author, "", "no author to count");
    assert_eq!(triage.issues[0].held, None);
    assert_eq!(triage.issues[0].calls, [], "no call to count");

    triage.issues.push(Triaged {
        issue: 13,
        comment: 0,
        step: TriageStep::Done,
        at: Timestamp(1_792_051_200),
        author: "newcomer".to_owned(),
        held: Some(TriageHeld::AuthorCap),
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: None,
        local: None,
        spam: None,
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([
            { "issue": 12, "comment": 5001, "at": 0, "step": "done" },
            {
                "issue": 13, "comment": 0, "at": 1_792_051_200, "author": "newcomer",
                "step": "done", "held": "author-cap",
            },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A record keeps the time of every call its issue made, oldest first, so the
// caps count an author's round beside the first pass's call. Past
// `TRIAGE_CALLS_KEPT` the oldest times go.
#[test]
fn a_record_keeps_the_time_of_every_call_its_issue_made() {
    let mut record = Triaged {
        issue: 12,
        comment: 5001,
        step: TriageStep::Done,
        at: Timestamp(1_792_051_200),
        author: "newcomer".to_owned(),
        held: None,
        calls: Vec::new(),
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
        asked: None,
        questions: None,
        local: None,
        spam: None,
    };
    for n in 0..(TRIAGE_CALLS_KEPT + 3) {
        record.add_call(Timestamp(1_792_051_200 + n as u64));
    }

    assert_eq!(record.calls.len(), TRIAGE_CALLS_KEPT);
    assert_eq!(
        record.calls.first().cloned().unwrap(),
        Timestamp(1_792_051_200 + 3),
        "the oldest calls went first"
    );
    assert_eq!(
        record.calls.last().cloned().unwrap(),
        Timestamp(1_792_051_200 + TRIAGE_CALLS_KEPT as u64 + 2)
    );
}
