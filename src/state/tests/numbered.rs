//! A state file saved while rulings had counter ids

use std::fs;

use serde_json::{Value, json};

use super::super::*;
use super::{a_work_item, store_in, ticket};
use crate::ticket::TicketId;

// A version 37 file: the work item for issue 42 and its pull request 51 is
// parked on ruling 40, which `rulings` and `heard` name by its counter id.
fn saved_before(dir: &Path, edit: impl FnOnce(&mut Value)) -> StateStore {
    let mut item = a_work_item();
    item.phase = Phase::Ruling { id: ticket(40) };
    let mut state = ProjectState::new();
    state.work_items = vec![item];
    let mut value = serde_json::to_value(&state).unwrap();
    value["version"] = json!(37);
    value["last_ruling"] = json!(41);
    value["rulings"] = json!([{
        "id": 40, "issue": 42, "question": "q", "pull_request": 51,
        "kind": { "kind": "stuck", "reason": "closed" },
    }]);
    value["heard"] = json!({
        "40": { "mentions": "octocat", "time": 6, "comment": 7, "ruling_comment": 5 },
    });
    edit(&mut value);
    fs::write(dir.join(STATE_FILE), value.to_string()).unwrap();
    store_in(dir)
}

#[test]
fn a_file_from_before_loads_with_its_open_rulings_named_by_number() {
    let dir = tempfile::tempdir().unwrap();
    let state = saved_before(dir.path(), |_| {}).load().unwrap().unwrap();
    let [ruling] = state.rulings.as_slice() else {
        panic!("{:?}", state.rulings);
    };
    assert_eq!((ruling.id, ruling.number()), (ticket(51), ticket(51)));
    assert_eq!(state.work_items[0].phase, Phase::Ruling { id: ticket(51) });
    assert_eq!(state.heard.keys().collect::<Vec<_>>(), [&ticket(51)]);
}

#[test]
fn an_open_ruling_before_a_pull_request_is_named_by_its_issue() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_before(dir.path(), |value| {
        value["rulings"][0]["pull_request"] = Value::Null;
        value["heard"] = json!({});
        value["unmentioned"] = json!({ "40": 99 });
    });
    let state = store.load().unwrap().unwrap();
    assert_eq!(state.rulings[0].id, ticket(42));
    assert_eq!(state.work_items[0].phase, Phase::Ruling { id: ticket(42) });
    assert_eq!(
        state.unmentioned.into_iter().collect::<Vec<_>>(),
        [(ticket(42), 99)]
    );
}

#[test]
fn the_renaming_is_saved_and_the_file_loads_whole_again() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_before(dir.path(), |_| {});
    let loaded = store.load().unwrap().unwrap();
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["rulings"][0]["id"], json!("51"));
    assert_eq!(
        saved["work_items"][0]["phase"],
        json!({ "state": "ruling", "id": "51" })
    );
    assert_eq!(
        (saved.get("last_ruling"), saved.get("renamed")),
        (None, None)
    );
    let mut again = store.load().unwrap().unwrap();
    assert_eq!(again, loaded);
    assert_eq!(
        again.take_renamed(),
        BTreeMap::new(),
        "nothing is left to rename"
    );
}

#[test]
fn an_answer_left_under_an_old_id_is_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = saved_before(dir.path(), |_| {}).load().unwrap().unwrap();
    assert_eq!(
        state.take_renamed(),
        BTreeMap::from([(ticket(40), ticket(51))])
    );
    assert_eq!(state.take_renamed(), BTreeMap::new());
    // A file this kelpie saved holds nothing to read.
    let store = store_in(dir.path());
    store.save(&state).unwrap();
    assert_eq!(
        store.load().unwrap().unwrap().take_renamed(),
        BTreeMap::new()
    );
}

#[test]
fn a_file_saved_after_keeps_its_ids_as_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_before(dir.path(), |value| {
        value["version"] = json!(VERSION);
        value["rulings"][0]["id"] = json!(3);
        value["work_items"][0]["phase"] = json!({ "state": "ruling", "id": 3 });
    });
    let mut state = store.load().unwrap().unwrap();
    assert_eq!(state.rulings[0].id, ticket(3));
    assert_eq!(state.take_renamed(), BTreeMap::new());
}

#[test]
fn rulings_swapping_ids_with_numbers_are_each_renamed_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_before(dir.path(), |value| {
        // Counter 51 is the ruling on #70, and counter 40 the one on #51.
        value["work_items"][0]["phase"] = json!({ "state": "ruling", "id": 40 });
        value["rulings"] = json!([
            { "id": 51, "issue": 70, "question": "a", "pull_request": null,
              "kind": { "kind": "stuck", "reason": "closed" } },
            { "id": 40, "issue": 42, "question": "b", "pull_request": 51,
              "kind": { "kind": "stuck", "reason": "closed" } },
        ]);
        value["heard"] = json!({
            "51": { "mentions": "a", "time": 1, "comment": 1 },
            "40": { "mentions": "b", "time": 2, "comment": 2 },
        });
    });
    let mut state = store.load().unwrap().unwrap();
    let named: Vec<(TicketId, &str)> = state
        .rulings
        .iter()
        .map(|r| (r.id, r.question.as_str()))
        .collect();
    assert_eq!(named, [(ticket(70), "a"), (ticket(51), "b")]);
    assert_eq!(state.heard[&ticket(70)].mentions, "a");
    assert_eq!(state.heard[&ticket(51)].mentions, "b");
    assert_eq!(state.work_items[0].phase, Phase::Ruling { id: ticket(51) });
    assert_eq!(
        state.take_renamed(),
        BTreeMap::from([(ticket(51), ticket(70)), (ticket(40), ticket(51))])
    );
}

#[test]
fn of_two_rulings_on_a_number_the_one_a_work_item_waits_on_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_before(dir.path(), |value| {
        let kind = json!({ "kind": "stuck", "reason": "closed" });
        value["rulings"] = json!([
            { "id": 40, "issue": 42, "question": "waited on", "pull_request": 51, "kind": kind },
            { "id": 41, "issue": 42, "question": "left over", "pull_request": 51, "kind": kind },
        ]);
    });
    let state = store.load().unwrap().unwrap();
    let named: Vec<(TicketId, &str)> = state
        .rulings
        .iter()
        .map(|r| (r.id, r.question.as_str()))
        .collect();
    assert_eq!(
        named,
        [(ticket(51), "waited on"), (ticket(41), "left over")]
    );
    assert_eq!(state.work_items[0].phase, Phase::Ruling { id: ticket(51) });
}
