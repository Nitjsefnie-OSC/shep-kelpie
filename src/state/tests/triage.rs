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
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["triage"]["issues"],
        serde_json::json!([{ "issue": 12, "comment": 5001, "step": "labelled" }])
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
            labels: Vec::new(),
            duplicate_of: None,
            ready_body: None,
            seen: Vec::new(),
        }]
    );

    triage.issues.push(Triaged {
        issue: 13,
        comment: 5002,
        step: TriageStep::Closed,
        labels: Vec::new(),
        duplicate_of: Some(41),
        ready_body: None,
        seen: Vec::new(),
    });
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(
        saved["triage"]["issues"][1],
        serde_json::json!({
            "issue": 13, "comment": 5002, "step": "closed", "duplicate_of": 41
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
        labels: Vec::new(),
        duplicate_of: None,
        ready_body: None,
        seen: Vec::new(),
    };
    let mut triage = TriageState {
        since: Timestamp(1),
        issues: vec![record(1, TriageStep::Commented)],
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
                labels: Vec::new(),
                duplicate_of: match issue {
                    2 => Some(41),
                    _ => None,
                },
                ready_body: None,
                seen: Vec::new(),
            })
            .collect(),
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
            { "issue": 12, "comment": 5001, "step": "commented", "labels": ["bug", "P2"] },
            { "issue": 13, "comment": 5002, "step": "labelled" },
            { "issue": 14, "comment": 5003, "step": "done" },
            { "issue": 15, "comment": 5004, "step": "closed", "duplicate_of": 41 },
        ])
    );
    assert_eq!(store.load().unwrap(), Some(loaded));
}
