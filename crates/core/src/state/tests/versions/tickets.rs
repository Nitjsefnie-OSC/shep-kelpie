//! The state file's keys are ticket ids: an older file's issue numbers load
//! as the GitHub id of the number, and a local ticket's id is written and
//! read back as itself

use std::fs;

use crate::state::tests::store_in;
use crate::state::*;
use crate::test::a_work_item;

// A version 71 or 72 file keys its work by issue numbers. Each one loads as
// the ticket id of that number, and saves as the id itself, the same id the
// number named.
#[test]
fn an_older_file_loads_its_issue_numbers_as_ticket_ids_and_saves_at_the_current_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    for version in [71, 72] {
        let mut old = serde_json::to_value(ProjectState::new()).unwrap();
        old["version"] = version.into();
        let mut item = serde_json::to_value(a_work_item()).unwrap();
        item["phase"] = serde_json::json!({ "state": "ruling", "id": 51 });
        old["work_items"] = serde_json::json!([item]);
        old["rulings"] = serde_json::json!([{
            "id": 51,
            "issue": 42,
            "question": "q",
            "pull_request": 51,
            "kind": { "kind": "stuck", "reason": "closed" },
            "alerted": true,
        }]);
        old["finished"] = serde_json::json!([22, 30]);
        old["dropped"] = serde_json::json!([7]);
        old["order"] = serde_json::json!([7, 3, 9]);
        old["ranked"] = serde_json::json!({ "7": 1 });
        old["history"] = serde_json::json!([{
            "issue": 22,
            "title": "A title",
            "pull_request": null,
            "merged": false,
            "at": 1,
            "wall": 0,
            "seconds": {},
        }]);
        old["leases"] = serde_json::json!([{ "resource": "gpu", "issue": 22, "since": 8 }]);
        old["notices"] = serde_json::json!([{ "issue": 22, "pull_request": 30, "head": "c0ffee" }]);
        fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

        let loaded = store.load().unwrap().expect("a saved state");
        store.save(&loaded).unwrap();
        let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        let id = |number: u64| serde_json::json!(number.to_string());
        assert_eq!(saved["version"], VERSION, "version {version}");
        assert_eq!(
            saved["work_items"][0]["issue"],
            id(42),
            "the work item's issue"
        );
        assert_eq!(
            saved["work_items"][0]["phase"]["id"],
            id(51),
            "the ruling it parks on"
        );
        assert_eq!(saved["rulings"][0]["id"], id(51), "the ruling's id");
        assert_eq!(
            saved["rulings"][0]["issue"],
            id(42),
            "the ruling's work item"
        );
        assert_eq!(
            saved["rulings"][0]["pull_request"],
            serde_json::json!(51),
            "a pull request stays a number"
        );
        assert_eq!(saved["finished"], serde_json::json!([id(22), id(30)]));
        assert_eq!(saved["dropped"], serde_json::json!([id(7)]));
        assert_eq!(saved["order"], serde_json::json!([id(7), id(3), id(9)]));
        assert_eq!(saved["ranked"], serde_json::json!({ "7": 1 }));
        assert_eq!(saved["history"][0]["issue"], id(22));
        assert_eq!(saved["leases"][0]["issue"], id(22));
        assert_eq!(saved["notices"][0]["issue"], id(22));
        // The same state loads back from the file the new kelpie wrote.
        assert_eq!(store.load().unwrap(), Some(loaded), "version {version}");
    }
}

// A local ticket has no issue number anywhere in the file: its `k12` is the
// work item's key, the ruling's name, and the board's entry, written and read
// back as itself.
#[test]
fn a_local_ticket_id_round_trips_through_the_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let local: TicketId = "k12".parse().unwrap();
    let mut item = a_work_item();
    item.issue = local;
    item.pull_request = None;
    item.branch = "kelpie/k12".to_owned();
    item.phase = Phase::Ruling { id: local };
    let ruling = Ruling {
        issue: Some(local),
        ..Ruling::new(local, "q", RulingKind::from(Stuck::Closed))
    };
    let mut state = ProjectState::new();
    state.work_items.push(item.clone());
    state.rulings.push(ruling);
    state.order = vec![local];
    state.finished = vec![local];
    state.dropped = vec![local];
    store.save(&state).unwrap();

    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(text.contains("\"k12\""), "{text}");
    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded, state);
    assert_eq!(
        loaded.item(local).map(|i| i.issue),
        Some(local),
        "found by its id"
    );
    assert_eq!(loaded.rulings[0].id, local);
    assert_eq!(loaded.rulings[0].issue, Some(local));
    assert_eq!(loaded.open_tickets(), vec![local]);
    // Saved again, it is the same file: the id is written as it parses.
    store.save(&loaded).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join(STATE_FILE)).unwrap(),
        text
    );
}
