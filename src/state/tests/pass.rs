use std::fs;

use super::super::*;
use super::store_in;
use crate::settings::AgentName;

// A version 50 file has no `pass`, `last_answered` or `local_failures` on a
// contribution, so it loads with none; they are kept once set, and left out
// while they are not.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 50.into();
    old["contributions"] = serde_json::json!([
        { "pull_request": 12, "author": "newcomer", "head": "2a8f1e6", "seen": 1 }
    ]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions[0].pass, None);
    assert_eq!(loaded.contributions[0].last_answered, None);
    store.save(&loaded).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["version"], VERSION);
    for key in ["pass", "last_answered", "local_failures"] {
        assert_eq!(saved["contributions"][0].get(key), None, "{key}");
    }

    let mut state = loaded;
    let name = AgentName::kelpies("qwen");
    state.contributions[0].last_answered = Some(name.clone());
    state.contributions[0].pass = Some(Pass {
        head: "2a8f1e6".into(),
        ran: vec![name.clone()],
        answered: Some(name),
        unread: None,
        findings: Vec::new(),
    });
    store.save(&state).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["contributions"][0]["last_answered"], "qwen");
    assert_eq!(saved["contributions"][0]["pass"]["answered"], "qwen");
    assert_eq!(store.load().unwrap(), Some(state));
}
