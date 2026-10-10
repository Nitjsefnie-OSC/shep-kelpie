use std::fs;

use super::super::*;

#[test]
fn where_a_ruling_is_answered_in_its_thread_is_pinned_by_ruling_id_and_left_out_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::new(dir.path().join("state.json"));
    let mut state = ProjectState::new();
    let saved = |store: &StateStore, state: &ProjectState| {
        store.save(state).unwrap();
        let text = fs::read_to_string(dir.path().join("state.json")).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()
    };
    let value = saved(&store, &state);
    assert_eq!((value.get("heard"), value.get("posted")), (None, None));

    state.heard.insert(
        2,
        Heard {
            mentions: "octocat".into(),
            time: Timestamp(6),
            comment: 7,
        },
    );
    state.posted = vec![9, 11];

    let value = saved(&store, &state);
    assert_eq!(
        value["heard"],
        serde_json::json!({ "2": { "mentions": "octocat", "time": 6, "comment": 7 } })
    );
    assert_eq!(value["posted"], serde_json::json!([9, 11]));
    assert_eq!(store.load().unwrap(), Some(state));
}

#[test]
fn a_login_is_mentioned_in_any_case() {
    let heard = Heard {
        mentions: "OctoCat".into(),
        time: Timestamp(0),
        comment: 0,
    };
    assert!(heard.is_mentioned("octocat") && !heard.is_mentioned("octocat2"));
}
