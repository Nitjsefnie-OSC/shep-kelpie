use std::fs;

use super::super::*;
use super::store_in;
use crate::ports::Timestamp;

// A version 57 file has no `answered` or `deciding` on a contribution, so it
// loads with nothing answered and no decision open; each is kept once set and
// left out while empty.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 57.into();
    old["contributions"] = serde_json::json!([
        { "pull_request": 12, "author": "newcomer", "head": "2a8f1e6", "seen": 1 }
    ]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions[0].answered, []);
    assert_eq!(loaded.contributions[0].deciding, None);
    store.save(&loaded).unwrap();
    let saved = read_back(dir.path());
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["contributions"][0].get("answered"), None);
    assert_eq!(saved["contributions"][0].get("deciding"), None);

    let mut state = loaded;
    let said = Said::Comment {
        id: 7,
        at: Timestamp(40),
    };
    state.contributions[0].answer(said.clone());
    state.contributions[0].deciding = Some(Deciding {
        said,
        asked: "May the API change?".into(),
        answer: None,
    });
    store.save(&state).unwrap();
    let saved = read_back(dir.path());
    assert_eq!(
        saved["contributions"][0]["answered"],
        serde_json::json!([{ "kind": "comment", "id": 7, "at": 40 }])
    );
    assert_eq!(
        saved["contributions"][0]["deciding"],
        serde_json::json!({
            "said": { "kind": "comment", "id": 7, "at": 40 },
            "asked": "May the API change?"
        })
    );
    assert_eq!(store.load().unwrap(), Some(state));
}

// A ruling that asks for the contributor's decision is saved by reason
#[test]
fn a_decision_ruling_is_saved_by_its_reason() {
    let kind = RulingKind::Contribution(Held::Question {
        asked: "May the API change?".into(),
    });
    let value = serde_json::to_value(&kind).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "kind": "contribution", "reason": "question", "asked": "May the API change?" })
    );
    assert_eq!(serde_json::from_value::<RulingKind>(value).unwrap(), kind);
}

fn read_back(dir: &std::path::Path) -> serde_json::Value {
    let text = fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    serde_json::from_str(&text).unwrap()
}
