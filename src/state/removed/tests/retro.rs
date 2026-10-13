//! A state file saved while a work item's worker was asked for its own retro

use std::fs;

use serde_json::json;

use super::saved_with;

#[test]
fn a_work_items_retro_mark_is_dropped_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_with(dir.path(), |value| {
        value["work_items"][0]["retro"] = json!(true);
    });
    let state = store.load().unwrap().unwrap();
    store.save(&state).unwrap();
    let saved = fs::read_to_string(dir.path().join("state.json")).unwrap();
    assert!(!saved.contains("\"retro\""), "{saved}");
    assert_eq!(store.load().unwrap().unwrap(), state);
}
