use std::fs;

use super::super::*;
use super::store_in;

// A version 48 file has no `spend` or `budget_raises` on a contribution, so it
// loads with nothing spent and no cap raised; both are kept once set, and left
// out while they are not.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 48.into();
    old["contributions"] = serde_json::json!([
        { "pull_request": 12, "author": "newcomer", "head": "2a8f1e6", "seen": 1 }
    ]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions[0].spend, Cost(0));
    assert_eq!(loaded.contributions[0].budget_raises, 0);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["contributions"][0].get("spend"), None);
    assert_eq!(saved["contributions"][0].get("budget_raises"), None);

    let mut state = loaded;
    state.contributions[0].spend = Cost(3_000_000_000);
    state.contributions[0].budget_raises = 2;
    store.save(&state).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["contributions"][0]["spend"], 3_000_000_000_u64);
    assert_eq!(saved["contributions"][0]["budget_raises"], 2);
    assert_eq!(store.load().unwrap(), Some(state));
}
