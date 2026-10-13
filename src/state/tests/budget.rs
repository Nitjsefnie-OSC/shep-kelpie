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

// What a dropped contribution spent is kept for when it comes back, given up
// once it is followed again, and bounded to the last `KEPT`.
#[test]
fn a_dropped_contributions_spend_is_kept_until_it_is_followed_again() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let now = Timestamp(1);
    let mut state = ProjectState::new();
    let mut spent = Contribution::new(12, "newcomer".into(), "2a8f1e6".into(), now);
    spent.spend = Cost(3_000_000_000);
    spent.budget_raises = 1;
    let free = Contribution::new(13, "newcomer".into(), "9c4d0b2".into(), now);
    state.contributions = vec![spent, free];

    state.follow(Vec::new());
    assert_eq!(state.contributions, Vec::new());
    assert_eq!(state.spent.len(), 1, "a free one has nothing to keep");
    store.save(&state).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["spent"],
        serde_json::json!([{ "pull_request": 12, "spend": 3_000_000_000_u64, "budget_raises": 1 }])
    );
    assert_eq!(store.load().unwrap(), Some(state.clone()));

    let back = Contribution::new(12, "newcomer".into(), "5e7a3c1".into(), now)
        .with_spent(state.spent_by(12));
    assert_eq!(back.spend, Cost(3_000_000_000));
    assert_eq!(back.budget_raises, 1);
    state.follow(vec![back]);
    assert_eq!(state.spent, Vec::new());

    for n in 100..160 {
        let mut c = Contribution::new(n, "newcomer".into(), "2a8f1e6".into(), now);
        c.spend = Cost(1);
        state.follow(vec![c]);
    }
    assert_eq!(state.spent.len(), KEPT);
    assert!(state.spent_by(158).is_some());
    assert!(state.spent_by(100).is_none(), "the oldest went first");
}
