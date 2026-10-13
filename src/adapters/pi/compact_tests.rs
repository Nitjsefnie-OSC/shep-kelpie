// What a pi call reports of its context and its compactions, and the call
// that compacts a session.

use serde_json::json;

use super::guard_tests::has_node;
use super::tests::{URL, WORKER_ID, World, id, output, stand_in, strings};
use super::*;

// Not a recording: written from pi's `json.md`, a threshold compaction in the
// middle of a turn, until a live recording replaces it (#762).
const COMPACTION: &str = include_str!("../../../fixtures/pi-p-compaction.jsonl");

fn header() -> String {
    json!({ "type": "session", "version": 3, "id": WORKER_ID }).to_string()
}

fn assistant(usage: serde_json::Value) -> String {
    json!({ "type": "message_end", "message": {
        "role": "assistant",
        "content": [{ "type": "text", "text": "done" }],
        "usage": usage,
        "stopReason": "stop",
    } })
    .to_string()
}

fn lines(lines: &[String]) -> String {
    lines.join("\n") + "\n"
}

fn tokens(input: u64, output: u64, cache_read: u64, cache_write: u64) -> serde_json::Value {
    json!({ "input": input, "output": output, "cacheRead": cache_read, "cacheWrite": cache_write })
}

fn turn(stdout: &str) -> AgentReply {
    parse_result(&output(0, stdout, ""), &id(WORKER_ID)).unwrap()
}

#[test]
fn a_turn_s_context_is_the_last_assistant_message_s_input_and_cache_tokens() {
    let stdout = lines(&[
        header(),
        assistant(tokens(100, 10, 0, 0)),
        assistant(tokens(300, 20, 50, 7)),
    ]);
    let reply = turn(&stdout);
    assert_eq!(reply.context, Some(357));
    assert_eq!(reply.usage.input, 400);
}

#[test]
fn a_turn_whose_messages_carry_no_usage_reports_no_context() {
    let bare = json!({ "type": "message_end", "message": {
        "role": "assistant",
        "content": [{ "type": "text", "text": "done" }],
        "stopReason": "stop",
    } })
    .to_string();
    let reply = turn(&lines(&[header(), bare]));
    assert_eq!(reply.context, None);
}

#[test]
fn a_later_message_with_no_usage_leaves_no_stale_context() {
    let bare = json!({ "type": "message_end", "message": {
        "role": "assistant",
        "content": [{ "type": "text", "text": "done" }],
        "stopReason": "stop",
    } })
    .to_string();
    let reply = turn(&lines(&[header(), assistant(tokens(300, 20, 50, 7)), bare]));
    assert_eq!(reply.context, None);
    assert_eq!(reply.usage.input, 300);
}

#[test]
fn a_compaction_event_with_an_odd_field_still_counts_its_usage() {
    let odd = COMPACTION.replace(r#""willRetry":true"#, r#""willRetry":"yes""#);
    assert_ne!(odd, COMPACTION);
    assert_eq!(turn(&odd).usage, turn(COMPACTION).usage);
}

#[test]
fn a_compaction_in_the_middle_of_a_turn_adds_its_usage_and_the_context_is_read_after_it() {
    let reply = turn(COMPACTION);
    assert_eq!(reply.context, Some(9_000 + 2_000 + 100));
    assert_eq!(
        reply.usage,
        Usage {
            // The turn's two requests, and the compaction's summary request
            input: 140_000 + 9_000 + 140_300,
            output: 300 + 40 + 900,
            cache_read: 2_000,
            cache_write: 100,
            cache_write_5m: 0,
        }
    );
    assert_eq!(reply.text, "Fixed.");
}

#[test]
fn a_compaction_that_aborted_adds_nothing() {
    let aborted = COMPACTION.replace(r#""aborted":false"#, r#""aborted":true"#);
    assert_eq!(turn(&aborted).usage.input, 140_000 + 9_000);
}

// What a stand-in pi prints for a compact call that ended with `end`.
fn compaction_end(end: serde_json::Value) -> String {
    let mut end = end;
    end["type"] = "compaction_end".into();
    end["reason"] = "manual".into();
    lines(&[
        header(),
        json!({ "type": "compaction_start", "reason": "manual" }).to_string(),
        end.to_string(),
    ])
}

// A worker call asking its session to compact.
fn compact_call(w: &World) -> AgentCall {
    let mut call = w.call(URL, Role::Worker, Session::Resume(id(WORKER_ID)));
    call.prompt = COMPACT.to_owned();
    call
}

fn compacted(stdout: &str) -> Result<AgentReply, AgentError> {
    let w = World::new();
    read(&compact_call(&w), &output(0, stdout, ""))
}

#[test]
fn a_compact_call_sends_the_extension_s_command_and_loads_the_extension_and_skips_the_skill_mapping()
 {
    let w = World::new();
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::Resume(id(WORKER_ID)));
    std::fs::create_dir_all(w.path("plugin/skills")).unwrap();
    call.plugin_dirs = vec![w.path("plugin")];
    call.prompt = COMPACT.to_owned();
    let argv = strings(&call);
    assert_eq!(argv.last().unwrap(), "/kelpie-compact");
    assert_eq!(argv[argv.len() - 2], "--");
    let extensions: Vec<&String> = argv
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--extension")
        .map(|(at, _)| &argv[at + 1])
        .collect();
    assert_eq!(
        extensions,
        [
            w.path("settings.guard.ts").to_str().unwrap(),
            w.path("settings.compact.ts").to_str().unwrap(),
        ]
    );
    assert!(!argv.iter().any(|a| a == "--skill"), "{argv:?}");

    call.prompt = "implement #7".into();
    let argv = strings(&call);
    assert!(!argv.iter().any(|a| a.ends_with("settings.compact.ts")));
    assert!(argv.iter().any(|a| a == "--skill"), "{argv:?}");
    assert_eq!(argv.last().unwrap(), "implement #7");
}

// A carried compact call is read for its compaction, not as a turn.
#[test]
fn a_carried_compact_call_is_read_for_its_compaction() {
    let end = json!({ "result": { "summary": "s" }, "aborted": false, "willRetry": false });
    let stdout = compaction_end(end);
    let reply = read_as(true, &output(0, &stdout, ""), &id(WORKER_ID)).unwrap();
    assert_eq!(reply.text, "");
    let turn = read_as(false, &output(0, &stdout, ""), &id(WORKER_ID));
    assert!(
        matches!(turn, Err(AgentError::Unreadable(Harness::Pi, _))),
        "{turn:?}"
    );
}

// The project manager's compaction is a compact call on a role that is no worker's.
#[test]
fn a_compact_call_of_any_role_gets_the_extension_and_another_call_of_that_role_none() {
    let w = World::new();
    let pi = stand_in(&w, COMPACTION);
    let mut call = w.call(URL, Role::Reviewer, Session::Resume(id(WORKER_ID)));
    pi.prepare(&call).unwrap();
    assert!(!w.path("settings.compact.ts").exists());
    call.prompt = COMPACT.to_owned();
    pi.prepare(&call).unwrap();
    assert!(w.path("settings.compact.ts").exists());
}

#[test]
fn a_worker_call_gets_the_compact_extension_beside_the_guard() {
    let w = World::new();
    let mut call = w.call(URL, Role::Worker, Session::New(id(WORKER_ID)));
    call.compact_window = std::num::NonZeroU32::new(200_000);
    stand_in(&w, COMPACTION).prepare(&call).unwrap();
    let extension = std::fs::read_to_string(w.path("settings.compact.ts")).unwrap();
    assert!(extension.contains("kelpie-compact"), "{extension}");
}

#[test]
fn a_compact_call_that_compacted_replies_with_its_usage_and_no_text() {
    let end = json!({
        "result": { "summary": "s", "usage": tokens(5_000, 400, 30, 2) },
        "aborted": false,
        "willRetry": false,
    });
    let reply = compacted(&compaction_end(end)).unwrap();
    assert_eq!(reply.session_id, id(WORKER_ID));
    assert_eq!(reply.text, "");
    assert_eq!(reply.context, None);
    assert_eq!(
        reply.usage,
        Usage {
            input: 5_000,
            output: 400,
            cache_read: 30,
            cache_write: 2,
            cache_write_5m: 0,
        }
    );
}

#[test]
fn a_compaction_that_aborted_or_failed_is_a_failed_call_that_says_why() {
    let aborted = json!({ "result": null, "aborted": true, "willRetry": false });
    let err = compacted(&compaction_end(aborted)).unwrap_err();
    assert!(
        matches!(&err, AgentError::Failed(Harness::Pi, why) if why.contains("aborted")),
        "{err:?}"
    );

    let failed = json!({
        "result": null,
        "aborted": false,
        "willRetry": false,
        "errorMessage": "summarization failed: the model said no",
    });
    let err = compacted(&compaction_end(failed)).unwrap_err();
    assert!(
        matches!(&err, AgentError::Failed(Harness::Pi, why)
            if why.contains("the model said no")),
        "{err:?}"
    );

    let empty = json!({ "result": null, "aborted": false, "willRetry": false });
    let err = compacted(&compaction_end(empty)).unwrap_err();
    assert!(matches!(err, AgentError::Failed(Harness::Pi, _)), "{err:?}");
}

#[test]
fn a_compact_call_with_no_compaction_event_is_unreadable() {
    let err = compacted(&lines(&[header()])).unwrap_err();
    assert!(
        matches!(err, AgentError::Unreadable(Harness::Pi, _)),
        "{err:?}"
    );
}

#[test]
fn a_compact_call_that_exits_badly_fails_as_any_pi_call_does() {
    let w = World::new();
    let call = compact_call(&w);
    let err = super::compact::parse(&output(1, &header(), "boom"), call.session.id()).unwrap_err();
    assert_eq!(err, AgentError::Failed(Harness::Pi, "boom".into()));
    let down = output(1, "", "503 Service Unavailable");
    let err = super::compact::parse(&down, call.session.id()).unwrap_err();
    assert!(
        matches!(err, AgentError::Unreachable(Harness::Pi, _)),
        "{err:?}"
    );
}

// Loads the extension as pi does, into a stand-in `pi`, and runs its command
// the way pi does: with a context whose `compact` ends as `ending` says.
const COMMAND_RUNNER: &str = r#"
import extension from "./compact.mts";
let command;
extension({ registerCommand: (name, options) => { command = { name, ...options }; } });
const log = [];
const ctx = {
  compact: (options) => {
    log.push("compact");
    setTimeout(() => {
      log.push("ended");
      if (process.argv[2] === "fail") options.onError(new Error("no summary"));
      else options.onComplete({ summary: "s" });
    }, 20);
  },
};
let outcome = "returned";
try {
  await command.handler("", ctx);
} catch (e) {
  outcome = `threw ${e.message}`;
}
log.push(outcome);
console.log(JSON.stringify({ name: command.name, log }));
"#;

fn commanded(ending: &str) -> serde_json::Value {
    let w = World::new();
    std::fs::write(w.path("compact.mts"), super::compact::extension(None)).unwrap();
    std::fs::write(w.path("run.mjs"), COMMAND_RUNNER).unwrap();
    let out = std::process::Command::new("node")
        .arg(w.path("run.mjs"))
        .arg(ending)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_compact_command_waits_for_the_compaction_and_fails_when_it_errors() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let done = commanded("complete");
    assert_eq!(done["name"], "kelpie-compact");
    assert_eq!(done["log"], json!(["compact", "ended", "returned"]));
    let failed = commanded("fail");
    assert_eq!(
        failed["log"],
        json!(["compact", "ended", "threw no summary"])
    );
}
