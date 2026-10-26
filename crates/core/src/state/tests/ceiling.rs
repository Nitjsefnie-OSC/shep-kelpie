use std::fs;

use super::super::*;
use super::store_in;
use crate::test::a_work_item;

// A version 73 file has no `ceiling_resumes` on a work item: none resumed. A
// count saves once above 0 and is left out at 0.
#[test]
fn a_state_file_from_before_ceiling_resumes_loads_with_none() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 73.into();
    old["work_items"] = serde_json::json!([serde_json::to_value(a_work_item()).unwrap()]);
    assert_eq!(old["work_items"][0].get("ceiling_resumes"), None);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let mut loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].ceiling_resumes, 0);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["work_items"][0].get("ceiling_resumes"), None);

    loaded.work_items[0].ceiling_resumes = 2;
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["work_items"][0]["ceiling_resumes"], 2);
    assert_eq!(store.load().unwrap(), Some(loaded));
}
