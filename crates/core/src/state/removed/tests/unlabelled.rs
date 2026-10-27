//! A file holding pending `unlabelled` rulings

use super::*;
use crate::state::VERSION;

// The previous build's file: an `unlabelled` ruling on #42, which no work item
// holds, its thread read and its comment posted, beside a `closed` one on #50.
fn saved_unlabelled(dir: &Path) -> StateStore {
    let mut value = serde_json::to_value(ProjectState::new()).unwrap();
    value["version"] = json!(VERSION);
    value["rulings"] = json!([
        {
            "id": 42, "issue": 42, "question": "q",
            "kind": { "kind": "stuck", "reason": "unlabelled", "why": "its call failed" },
        },
        {
            "id": 51, "issue": 50, "question": "q", "pull_request": 51,
            "kind": { "kind": "stuck", "reason": "closed" },
        },
    ]);
    value["heard"] = json!({
        "42": { "mentions": "octocat", "time": 6, "comment": 7, "ruling_comment": 5 },
        "51": { "mentions": "octocat", "time": 6, "comment": 8, "ruling_comment": 4 },
    });
    value["unmentioned"] = json!({ "42": 99 });
    fs::write(dir.join(STATE_FILE), value.to_string()).unwrap();
    store_in(dir)
}

#[test]
fn a_pending_unlabelled_ruling_is_withdrawn_and_the_rest_load() {
    let dir = tempfile::tempdir().unwrap();
    let state = saved_unlabelled(dir.path()).load().unwrap().unwrap();

    let ids: Vec<_> = state.rulings.iter().map(|r| r.id).collect();
    assert_eq!(ids, [ticket(51)]);
    assert_eq!(state.heard.keys().collect::<Vec<_>>(), [&ticket(51)]);
    assert!(state.unmentioned.is_empty(), "{:?}", state.unmentioned);
}

#[test]
fn a_file_with_its_unlabelled_ruling_withdrawn_saves_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = saved_unlabelled(dir.path());
    let state = store.load().unwrap().unwrap();

    store.save(&state).unwrap();

    let saved = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(!saved.contains("unlabelled"), "{saved}");
    assert_eq!(store.load().unwrap().unwrap().rulings.len(), 1);
}
