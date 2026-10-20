use std::fs;
use std::path::PathBuf;

use super::super::*;
use super::store_in;
use super::ticket;
use crate::ports::SessionId;
use crate::worktree::Remote;

fn item(issue: u64) -> WorkItem {
    WorkItem::new(
        ticket(issue),
        "A title",
        format!("kelpie/{issue}"),
        PathBuf::from("wt"),
        PathBuf::from("build"),
        "sonnet-high".to_owned().try_into().unwrap(),
        SessionId("s".to_owned()),
    )
}

// A version 62 file has no `remote` on a work item and no `left_open`, so it
// loads with the branch on origin and nothing left open; a fork is kept once
// set, and a branch on origin and an empty list are left out.
#[test]
fn an_older_state_file_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut old = serde_json::to_value(ProjectState::new()).unwrap();
    old["version"] = 62.into();
    old["work_items"] = serde_json::json!([item(7)]);
    fs::write(dir.path().join(STATE_FILE), old.to_string()).unwrap();

    let loaded = store.load().unwrap().expect("a saved state");
    assert_eq!(loaded.work_items[0].remote, Remote::Origin);
    assert_eq!(loaded.left_open, Vec::<u64>::new());
    store.save(&loaded).unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join(STATE_FILE)).unwrap()).unwrap();
    assert_eq!(saved["version"], VERSION);
    assert_eq!(saved["work_items"][0].get("remote"), None);
    assert_eq!(saved.get("left_open"), None);

    let mut state = loaded;
    state.work_items[0].remote = Remote::Fork {
        url: "https://github.com/newcomer/shep.git".to_owned(),
        branch: "patch-1".to_owned(),
    };
    state.left_open = vec![12];
    store.save(&state).unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join(STATE_FILE)).unwrap()).unwrap();
    assert_eq!(
        saved["work_items"][0]["remote"],
        serde_json::json!({
            "fork": { "url": "https://github.com/newcomer/shep.git", "branch": "patch-1" }
        })
    );
    assert_eq!(saved["left_open"], serde_json::json!([12]));
    assert_eq!(store.load().unwrap(), Some(state));
}

// The ruling for a fork without maintainer edits is saved by its reason
#[test]
fn a_no_edits_ruling_is_saved_by_its_reason() {
    let kind = RulingKind::Contribution(Held::NoEdits);
    let value = serde_json::to_value(&kind).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "kind": "contribution", "reason": "no-edits" })
    );
    assert_eq!(serde_json::from_value::<RulingKind>(value).unwrap(), kind);
}
