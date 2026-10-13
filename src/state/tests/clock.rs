use std::fs;

use super::super::*;
use super::store_in;
use crate::ports::Timestamp;

// A version 52 file has no `waiting_since` on a contribution, so it loads with
// no clock running; one is kept once set, and left out while it is not.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 52.into();
    old["contributions"] = serde_json::json!([
        { "pull_request": 12, "author": "newcomer", "head": "2a8f1e6", "seen": 1 }
    ]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions[0].waiting_since, None);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["contributions"][0].get("waiting_since"), None);

    let mut state = loaded;
    state.contributions[0].waiting_since = Some(Timestamp(90));
    store.save(&state).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["contributions"][0]["waiting_since"], 90);
    assert_eq!(store.load().unwrap(), Some(state));
}
