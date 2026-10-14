use std::fs;

use super::super::*;
use super::store_in;
use crate::ports::Timestamp;

// A version 58 file has no `asked` or `released` on a contribution, so it
// loads with nothing asked and nothing released; each is kept once set, and
// left out while it is not.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 58.into();
    old["contributions"] = serde_json::json!([
        { "pull_request": 12, "author": "newcomer", "head": "2a8f1e6", "seen": 1,
          "waiting_since": 5 }
    ]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.contributions[0].asked, None);
    assert_eq!(loaded.contributions[0].released, None);
    store.save(&loaded).unwrap();
    let saved = read_back(dir.path());
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["contributions"][0].get("asked"), None);
    assert_eq!(saved["contributions"][0].get("released"), None);

    let mut state = loaded;
    state.contributions[0].asked = Some(Asked {
        comment: 7000,
        at: Timestamp(60),
        asks: 2,
        nos: 1,
        read: Vec::new(),
        closing: false,
    });
    state.contributions[0].released = Some(Released {
        at: Timestamp(90),
        how: How::ThumbsUp,
    });
    store.save(&state).unwrap();
    let saved = read_back(dir.path());
    assert_eq!(
        saved["contributions"][0]["asked"],
        serde_json::json!({ "comment": 7000, "at": 60, "asks": 2, "nos": 1 })
    );
    assert_eq!(
        saved["contributions"][0]["released"],
        serde_json::json!({ "at": 90, "how": "thumbs-up" })
    );
    assert_eq!(store.load().unwrap(), Some(state));
}

// The ruling for a contributor who said no twice is saved by its reason
#[test]
fn a_waiting_ruling_is_saved_by_its_reason() {
    let kind = RulingKind::Contribution(Held::Waiting);
    let value = serde_json::to_value(&kind).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "kind": "contribution", "reason": "waiting" })
    );
    assert_eq!(serde_json::from_value::<RulingKind>(value).unwrap(), kind);
}

fn read_back(dir: &std::path::Path) -> serde_json::Value {
    let text = fs::read_to_string(dir.join(STATE_FILE)).unwrap();
    serde_json::from_str(&text).unwrap()
}

// A push starts the asking over, but not the maintainer's close: the thanks
// and the close still follow, whatever the contributor pushes meanwhile.
#[test]
fn a_push_drops_an_ask_but_not_the_maintainers_close() {
    let ask = |closing| Asked {
        comment: 7000,
        at: Timestamp(60),
        asks: 1,
        nos: 0,
        read: Vec::new(),
        closing,
    };
    let mut held = Contribution::new(12, "newcomer".into(), "2a8f1e6".into(), Timestamp(1));

    held.asked = Some(ask(false));
    let pushed = held.pushed("9c0ffee".into(), Timestamp(70));
    assert_eq!(pushed.asked, None);

    held.asked = Some(ask(true));
    let pushed = held.pushed("9c0ffee".into(), Timestamp(70));
    assert_eq!(pushed.asked, Some(ask(true)));
}
