use std::fs;

use super::super::*;
use super::store_in;
use crate::ports::{Finding, Severity, Timestamp};
use crate::review_bot::Bot;
use crate::settings::AgentName;
use crate::work_item::ReviewStage;

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
    for key in ["pass", "walking", "last_answered", "local_failures"] {
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
        nits: Vec::new(),
    });
    store.save(&state).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["contributions"][0]["last_answered"], "qwen");
    assert_eq!(saved["contributions"][0]["pass"]["answered"], "qwen");
    assert_eq!(store.load().unwrap(), Some(state));
}

// A pass held at a review bot's turn, and a bot's nits, are kept across a
// restart.
#[test]
fn a_walk_at_a_bots_turn_and_its_nits_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    let mut state = ProjectState::new();
    let mut held = Contribution::new(12, "newcomer".into(), "2a8f1e6".into(), Timestamp(1));
    let cubic = AgentName::kelpies("cubic");
    held.walking = Some(Walking {
        head: "2a8f1e6".into(),
        past: vec![AgentName::kelpies("qwen")],
        ran: Vec::new(),
        missed: vec!["qwen was passed over".into()],
        bot: Some(BotTurn {
            reviewer: cubic,
            stage: ReviewStage::Summon {
                bot: Bot::Cubic,
                started: Timestamp(2),
                head: "2a8f1e6".into(),
                readied: None,
                full: false,
            },
        }),
    });
    state.contributions.push(held);
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap(), Some(state.clone()));

    let nit = Finding::new(Severity::Low, "a.rs", 3, "rename it", "");
    let pass = Pass {
        head: "2a8f1e6".into(),
        ran: Vec::new(),
        answered: None,
        unread: None,
        findings: Vec::new(),
        nits: vec![nit],
    };
    state.contributions[0].pass = Some(pass);
    state.contributions[0].walking = None;
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap(), Some(state));
}
