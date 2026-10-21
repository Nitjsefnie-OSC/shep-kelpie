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

    let upstream = PiCli::on(&Base::default())
        .upstream(server(&call).unwrap())
        .unwrap();
    let socket = w.path("forwarder.sock");
    let policy = policy(&call, &Files::of(&call), &upstream, &socket, &[]);
    assert!(policy.read.contains(&w.path("settings.prompt.md")));
}

// Loads the extension as pi does, into a stand-in `pi`, starts a run and
// fires a `session_compact` event for each reason in turn, then ends the run
// and fires a `threshold` one, as pi does after a last answer.
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
await handlers.agent_start({ type: "agent_start" });
for (const reason of ["threshold", "overflow", "manual"]) {
  sent.length = 0;
  await handlers.session_compact({ type: "session_compact", reason });
  out[reason] = [...sent];
}
await handlers.agent_end({ type: "agent_end" });
sent.length = 0;
await handlers.session_compact({ type: "session_compact", reason: "threshold" });
out.after_the_run = [...sent];
console.log(JSON.stringify(out));
"#;

// What the extension sent for each event of `COMPACTION_RUNNER`, or None
// without node
fn compaction_events() -> Option<serde_json::Value> {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return None;
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
    Some(serde_json::from_slice(&out.stdout).unwrap())
}

#[test]
fn the_extension_hands_the_prompt_back_after_a_threshold_or_overflow_compaction_and_not_after_a_manual_one()
 {
    let Some(sent) = compaction_events() else {
        return;
    };
    let again = json!([{
        "message": { "customType": "kelpie-prompt", "content": "the prompt again\n", "display": false },
        "options": { "deliverAs": "steer" },
    }]);
    assert_eq!(sent["threshold"], again);
    assert_eq!(sent["overflow"], again);
    assert_eq!(sent["manual"], json!([]));
}

// pi compacts after a run's last answer when the context is still past its
// threshold, and the prompt handed back then would start the task over.
#[test]
fn the_extension_hands_nothing_back_after_a_compaction_that_follows_the_last_answer() {
    let Some(sent) = compaction_events() else {
        return;
    };
    assert_eq!(sent["after_the_run"], json!([]));
}

// A stale `settings.json` from an earlier call on the item would keep its
// reserve, so a call that needs none removes it.
#[test]
fn a_call_that_needs_no_reserve_removes_the_one_an_earlier_call_left() {
    let w = World::new();
    let pi = stand_in(&w, FRESH);
    pi.prepare(&windowed(&w, 262_144, Some(200_000))).unwrap();
    assert!(settings_written(&w).is_some());
    for call in [
        windowed(&w, 64_000, Some(200_000)),
        windowed(&w, 262_144, None),
    ] {
        pi.prepare(&windowed(&w, 262_144, Some(200_000))).unwrap();
        pi.prepare(&call).unwrap();
        assert_eq!(settings_written(&w), None);
    }
}

#[test]
fn a_step_s_prompt_is_handed_back_as_the_skill_text_pi_ran_and_not_the_command() {
    let w = World::new();
    let plugin = w.path("plugins/shep");
    std::fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(plugin.join("skills/implement")).unwrap();
    std::fs::write(
        plugin.join(".claude-plugin/plugin.json"),
        r#"{"name":"shep"}"#,
    )
    .unwrap();
    std::fs::write(
        plugin.join("skills/implement/SKILL.md"),
        "---\nname: implement\ndescription: d\n---\nBuild it test first.\n",
    )
    .unwrap();
    let mut call = windowed(&w, 262_144, Some(200_000));
    call.plugin_dirs = vec![plugin];
    call.prompt = "/shep:implement Implement issue #7".into();
    stand_in(&w, FRESH).prepare(&call).unwrap();
    let prompt = std::fs::read_to_string(w.path("settings.prompt.md")).unwrap();
    assert!(prompt.contains("Build it test first."), "{prompt}");
    assert!(!prompt.contains("description: d"), "{prompt}");
    assert!(!prompt.contains("/shep:implement"), "{prompt}");
    assert!(prompt.ends_with("Implement issue #7\n"), "{prompt}");
}
