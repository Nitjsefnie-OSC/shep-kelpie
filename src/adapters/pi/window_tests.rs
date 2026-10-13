// `worker.compact_window` on pi: the reserve it maps to in pi's settings, and
// the prompt a mid-turn compaction hands back.

use std::num::NonZeroU32;

use serde_json::json;

use super::guard_tests::has_node;
use super::tests::{FRESH, URL, WORKER_ID, World, id, stand_in, strings};
use super::*;
use crate::settings::ContextSize;

// A worker call on a model with `context` tokens, bounded to `window`.
fn windowed(w: &World, context: i64, window: Option<u32>) -> AgentCall {
    let mut call = w.call(URL, Role::Worker, Session::New(id(WORKER_ID)));
    let AgentHarness::Pi(server) = &mut call.harness else {
        unreachable!("the world's calls run on pi")
    };
    server.context = ContextSize::try_from(context).unwrap();
    call.compact_window = window.and_then(NonZeroU32::new);
    call
}

// What pi reads from its own settings file in the call's home, if kelpie wrote one.
fn settings_written(w: &World) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(w.path("settings.pi/settings.json")).ok()?;
    Some(serde_json::from_str(&text).unwrap())
}

#[test]
fn a_window_below_the_models_context_sets_pis_reserve_tokens_to_the_difference() {
    let w = World::new();
    let call = windowed(&w, 262_144, Some(200_000));
    stand_in(&w, FRESH).prepare(&call).unwrap();
    assert_eq!(
        settings_written(&w),
        Some(json!({ "compaction": { "reserveTokens": 62_144 } }))
    );
}

#[test]
fn a_window_at_or_past_the_context_or_within_the_default_reserve_of_it_leaves_pis_default() {
    let cases = [
        // (context, window)
        (64_000, 200_000),
        (262_144, 262_144 + 1),
        (200_000, 190_000),
        (200_000, 200_000 - 16_384),
    ];
    for (context, window) in cases {
        let w = World::new();
        let call = windowed(&w, context, Some(window));
        stand_in(&w, FRESH).prepare(&call).unwrap();
        assert_eq!(settings_written(&w), None, "{context} with {window}");
    }
}

#[test]
fn a_call_with_no_compact_window_writes_no_settings_file() {
    let w = World::new();
    let call = windowed(&w, 262_144, None);
    stand_in(&w, FRESH).prepare(&call).unwrap();
    assert_eq!(settings_written(&w), None);
    assert!(!w.path("settings.prompt.md").exists());
}

#[test]
fn a_worker_with_a_window_writes_its_prompt_beside_the_settings_and_loads_the_extension() {
    let w = World::new();
    let mut call = windowed(&w, 262_144, Some(200_000));
    call.prompt = "implement #7".into();
    stand_in(&w, FRESH).prepare(&call).unwrap();

    let prompt = std::fs::read_to_string(w.path("settings.prompt.md")).unwrap();
    assert!(prompt.contains("pi compacted your session"), "{prompt}");
    assert!(prompt.ends_with("implement #7\n"), "{prompt}");

    let extension = std::fs::read_to_string(w.path("settings.compact.ts")).unwrap();
    let file = json!(w.path("settings.prompt.md")).to_string();
    assert!(extension.contains(&format!("PROMPT_FILE: string | null = {file};")));
    assert!(!extension.contains("__PROMPT_FILE__"), "{extension}");

    let argv = strings(&call);
    let compact = w.path("settings.compact.ts");
    let at = argv
        .iter()
        .position(|a| a.as_str() == compact.to_str().unwrap());
    assert_eq!(at.map(|at| argv[at - 1].as_str()), Some("--extension"));

    let upstream = ClaudeCli::default()
        .pi()
        .upstream(server(&call).unwrap())
        .unwrap();
    let socket = w.path("forwarder.sock");
    let policy = policy(&call, &Files::of(&call), &upstream, &socket, &[]);
    assert!(policy.read.contains(&w.path("settings.prompt.md")));
}

// Loads the extension as pi does, into a stand-in `pi`, and fires a
// `session_compact` event for each reason in turn.
const COMPACTION_RUNNER: &str = r#"
import extension from "./compact.mts";
const handlers = {};
const sent = [];
extension({
  registerCommand: () => {},
  on: (name, fn) => { handlers[name] = fn; },
  sendMessage: (message, options) => { sent.push({ message, options }); },
});
const out = {};
for (const reason of ["threshold", "overflow", "manual"]) {
  sent.length = 0;
  await handlers.session_compact({ type: "session_compact", reason });
  out[reason] = [...sent];
}
console.log(JSON.stringify(out));
"#;

#[test]
fn the_extension_hands_the_prompt_back_after_a_threshold_or_overflow_compaction_and_not_after_a_manual_one()
 {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    std::fs::write(w.path("prompt.md"), "the prompt again\n").unwrap();
    std::fs::write(
        w.path("compact.mts"),
        super::compact::extension(Some(&w.path("prompt.md"))),
    )
    .unwrap();
    std::fs::write(w.path("run.mjs"), COMPACTION_RUNNER).unwrap();
    let out = std::process::Command::new("node")
        .arg(w.path("run.mjs"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let sent: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let again = json!([{
        "message": { "customType": "kelpie-prompt", "content": "the prompt again\n", "display": false },
        "options": { "deliverAs": "steer" },
    }]);
    assert_eq!(sent["threshold"], again);
    assert_eq!(sent["overflow"], again);
    assert_eq!(sent["manual"], json!([]));
}
