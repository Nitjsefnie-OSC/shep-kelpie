use std::fs;

use super::super::*;

#[test]
fn the_place_read_to_in_a_rulings_thread_is_pinned_by_ruling_id_and_left_out_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::new(dir.path().join("state.json"));
    let mut state = ProjectState::new();
    let saved = |store: &StateStore, state: &ProjectState| {
        store.save(state).unwrap();
        let text = fs::read_to_string(dir.path().join("state.json")).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()
    };
    assert_eq!(saved(&store, &state).get("heard"), None);

    state.heard.insert(
        2,
        Heard {
            time: Timestamp(6),
            comment: 7,
        },
    );

    let value = saved(&store, &state);
    assert_eq!(
        value["heard"],
        serde_json::json!({ "2": { "time": 6, "comment": 7 } })
    );
    assert_eq!(store.load().unwrap(), Some(state));
}
