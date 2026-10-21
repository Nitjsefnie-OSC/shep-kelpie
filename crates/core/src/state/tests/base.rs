use std::fs;

use super::super::*;
use super::store_in;
use crate::test::a_work_item;

// A version 78 file has no `base` on a work item: the runner that loads it
// says which. It saves once set and is left out when absent.
#[test]
fn a_state_file_from_before_base_loads_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 78.into();
    old["work_items"] = serde_json::json!([serde_json::to_value(a_work_item()).unwrap()]);
    assert_eq!(old["work_items"][0].get("base"), None);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].base, None);

    loaded.work_items[0].base = Some("dev".to_owned());
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["work_items"][0]["base"], "dev");
    assert_eq!(store.load().unwrap(), Some(loaded));
}

// A version 77 file has no `contributor` on a work item: none is a
// contribution's. It saves once set and is left out when absent.
#[test]
fn a_state_file_from_before_contributor_loads_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 77.into();
    old["work_items"] = serde_json::json!([serde_json::to_value(a_work_item()).unwrap()]);
    assert_eq!(old["work_items"][0].get("contributor"), None);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].contributor, None);

    loaded.work_items[0].contributor = Some("newcomer".to_owned());
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["work_items"][0]["contributor"], "newcomer");
    assert_eq!(store.load().unwrap(), Some(loaded));
}
