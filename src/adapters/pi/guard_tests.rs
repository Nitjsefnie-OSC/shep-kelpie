// The guard extension a fenced pi call loads: what it holds, and what it
// decides for each tool call.

use std::path::Path;

use serde_json::json;

use super::tests::{FRESH, URL, WORKER_ID, World, id, stand_in};
use super::*;

#[test]
fn a_fenced_call_gets_a_guard_that_runs_kelpies_checks() {
    let w = World::new();
    let call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    stand_in(&w, FRESH).prepare(&call).unwrap();
    let guard = std::fs::read_to_string(w.path("settings.guard.ts")).unwrap();
    assert!(!guard.contains("__CHECKS__"), "{guard}");
    let (wt, build) = (w.path("wt"), w.path("build"));
    let line = guard
        .lines()
        .find(|l| l.starts_with("const CHECKS"))
        .unwrap();
    let checks: serde_json::Value =
        serde_json::from_str(line.split_once(" = ").unwrap().1.trim_end_matches(';')).unwrap();
    assert_eq!(checks["kelpie"], "/k/kelpie");
    assert_eq!(checks["confine"], json!(["confine", wt, build]));
    assert_eq!(checks["guard"][0], "guard");
    assert_eq!(checks["guard"][2], json!(wt));
}

// The project's hooks are not Claude Code's, so pi gets them as `kelpie
// guard` runs them.
#[test]
fn the_projects_hooks_reach_a_pi_workers_guard() {
    let w = World::new();
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let hook = crate::settings::ProjectHook {
        point: crate::settings::HookPoint::Commit,
        command: "check-words".to_owned().try_into().unwrap(),
    };
    let hooks = crate::hooks::Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &[hook]);
    let flag = hooks.flag().unwrap();
    call.reach.fence.as_mut().unwrap().guard.hooks = hooks;
    stand_in(&w, FRESH).prepare(&call).unwrap();

    let guard = std::fs::read_to_string(w.path("settings.guard.ts")).unwrap();
    let line = guard
        .lines()
        .find(|l| l.starts_with("const CHECKS"))
        .unwrap();
    let checks: serde_json::Value =
        serde_json::from_str(line.split_once(" = ").unwrap().1.trim_end_matches(';')).unwrap();
    assert!(checks["guard"].as_array().unwrap().contains(&json!(flag)));
}

// Loads the guard extension the way pi does, into a stand-in `pi`, and hands
// its `tool_call` handler each of `events` in turn.
const GUARD_RUNNER: &str = r#"
import guard from "./guard.mts";
let handler;
guard({ on: (name, fn) => { if (name === "tool_call") handler = fn; } });
const out = [];
for (const event of JSON.parse(process.argv[2])) {
  out.push((await handler(event, { cwd: process.argv[3] })) ?? null);
}
console.log(JSON.stringify(out));
"#;

// What the guard extension decides for each of `events`, with `kelpie` as its checks.
pub(super) fn guarded(
    w: &World,
    kelpie: &Path,
    events: serde_json::Value,
) -> Vec<serde_json::Value> {
    let call = w.fenced(URL, kelpie, Session::New(id(WORKER_ID)));
    let fence = call.reach.fence.as_deref().unwrap();
    std::fs::write(w.path("guard.mts"), guard_extension(fence)).unwrap();
    std::fs::write(w.path("run.mjs"), GUARD_RUNNER).unwrap();
    let out = std::process::Command::new("node")
        .arg(w.path("run.mjs"))
        .arg(events.to_string())
        .arg(w.path("wt"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

// A node that loads TypeScript as pi does, which an older one cannot.
pub(super) fn has_node() -> bool {
    std::process::Command::new("node")
        .args([
            "--input-type=module-typescript",
            "-e",
            "const x: number = 1",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn the_guard_sends_commands_to_guard_and_writes_to_confine_and_refuses_the_rest() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    // A stand-in for kelpie's checks: it logs each call and refuses `refuse-me`.
    let log = w.path("checks.log");
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> {log}\nin=$(cat)\necho \"$in\" >> {log}\n\
         case \"$in\" in *refuse-me*) echo 'no, says kelpie' >&2; exit 2;; esac\n",
        log = log.display()
    );
    crate::test::write_script(&w.path("kelpie"), &script);
    let events = json!([
        { "toolName": "bash", "input": { "command": "ls" } },
        { "toolName": "bash", "input": { "command": "refuse-me" } },
        { "toolName": "write", "input": { "path": "src/a.txt", "content": "" } },
        { "toolName": "edit", "input": { "path": "@b.txt", "edits": [] } },
        { "toolName": "write", "input": { "path": "file:///etc/hosts", "content": "" } },
        { "toolName": "read", "input": { "path": "c.txt" } },
        { "toolName": "webfetch", "input": { "url": "https://example.com" } },
    ]);
    let verdicts = guarded(&w, &w.path("kelpie"), events);
    let refused = |v: &serde_json::Value| v["block"] == true;
    let reasons: Vec<&str> = verdicts
        .iter()
        .map(|v| v["reason"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        verdicts.iter().map(refused).collect::<Vec<_>>(),
        [false, true, false, false, true, false, true]
    );
    assert_eq!(reasons[1], "no, says kelpie");
    assert!(reasons[4].contains("file: URL"), "{}", reasons[4]);
    assert!(reasons[6].contains("webfetch"), "{}", reasons[6]);

    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    let (wt, build) = (w.path("wt"), w.path("build"));
    let confine = format!("confine {} {}", wt.display(), build.display());
    // Each check's arguments, then the call it was handed: four checks in all.
    assert_eq!(lines.len(), 8, "{calls}");
    assert!(lines[0].starts_with("guard "), "{}", lines[0]);
    let first: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(
        first,
        json!({ "cwd": wt, "tool_name": "Bash", "tool_input": { "command": "ls" } })
    );
    assert_eq!(lines[4], confine);
    let write: serde_json::Value = serde_json::from_str(lines[5]).unwrap();
    assert_eq!(write["tool_name"], "Write");
    assert_eq!(
        write["tool_input"]["file_path"],
        json!(wt.join("src/a.txt"))
    );
    let edit: serde_json::Value = serde_json::from_str(lines[7]).unwrap();
    assert_eq!(edit["tool_name"], "Edit");
    assert_eq!(edit["tool_input"]["file_path"], json!(wt.join("b.txt")));
}

fn pi_hooks(points: &[crate::settings::HookPoint]) -> crate::hooks::Hooks {
    let list: Vec<crate::settings::ProjectHook> = points
        .iter()
        .map(|&point| crate::settings::ProjectHook {
            point,
            command: "check-words".to_owned().try_into().unwrap(),
        })
        .collect();
    crate::hooks::Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &list)
}

// The checks the guard extension is given for a fenced worker with `hooks`.
fn checks_for(w: &World, hooks: crate::hooks::Hooks) -> serde_json::Value {
    let mut call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    call.reach.fence.as_mut().unwrap().guard.hooks = hooks;
    stand_in(w, FRESH).prepare(&call).unwrap();
    let guard = std::fs::read_to_string(w.path("settings.guard.ts")).unwrap();
    let line = guard
        .lines()
        .find(|l| l.starts_with("const CHECKS"))
        .unwrap();
    serde_json::from_str(line.split_once(" = ").unwrap().1.trim_end_matches(';')).unwrap()
}

#[test]
fn the_projects_write_and_after_command_hooks_reach_a_pi_workers_checks() {
    use crate::settings::HookPoint;
    let hooks = pi_hooks(&[HookPoint::Write, HookPoint::AfterCommand]);
    let flag = hooks.flag().unwrap();

    let checks = checks_for(&World::new(), hooks);

    let confine = checks["confine"].as_array().unwrap();
    assert_eq!(confine.last().unwrap(), &json!(flag));
    let after = checks["after"].as_array().unwrap();
    assert_eq!(after[0], "guard");
    assert!(after.contains(&json!(flag)), "{after:?}");
    assert_eq!(after.last().unwrap(), "--after");

    let plain = checks_for(&World::new(), pi_hooks(&[HookPoint::Commit]));
    assert_eq!(plain["confine"].as_array().unwrap().len(), 3);
    assert_eq!(plain["after"], serde_json::Value::Null);
}

// Loads the guard extension as pi does and hands its `tool_result` handler each of `events`.
const RESULT_RUNNER: &str = r#"
import guard from "./guard.mts";
let handler;
guard({ on: (name, fn) => { if (name === "tool_result") handler = fn; } });
const out = [];
for (const event of JSON.parse(process.argv[2])) {
  out.push((await handler(event, { cwd: process.argv[3] })) ?? null);
}
console.log(JSON.stringify(out));
"#;

// What the guard extension answers each of `events` with, as pi's `tool_result` handler.
fn after_results(w: &World, kelpie: &Path, events: serde_json::Value) -> Vec<serde_json::Value> {
    use crate::settings::HookPoint;
    let mut call = w.fenced(URL, kelpie, Session::New(id(WORKER_ID)));
    call.reach.fence.as_mut().unwrap().guard.hooks = pi_hooks(&[HookPoint::AfterCommand]);
    let fence = call.reach.fence.as_deref().unwrap();
    std::fs::write(w.path("guard.mts"), guard_extension(fence)).unwrap();
    std::fs::write(w.path("run.mjs"), RESULT_RUNNER).unwrap();
    let out = std::process::Command::new("node")
        .arg(w.path("run.mjs"))
        .arg(events.to_string())
        .arg(w.path("wt"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

// pi has no PostToolUse: its `tool_result` event is where a refused result reaches the model.
#[test]
fn an_after_command_refusal_reaches_a_pi_worker_as_feedback_on_the_result() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    // A stand-in for kelpie's check: it logs the result it was handed and refuses `refuse-me`.
    let log = w.path("checks.log");
    let script = format!(
        "#!/bin/sh\necho \"$*\" >> {log}\nin=$(cat)\necho \"$in\" >> {log}\n\
         case \"$in\" in *refuse-me*) echo 'the build failed' >&2; exit 2;; esac\n",
        log = log.display()
    );
    crate::test::write_script(&w.path("kelpie"), &script);
    let ok = json!([{ "type": "text", "text": "fine" }]);
    let bad = json!([{ "type": "text", "text": "refuse-me. Command exited with code 101" }]);
    let events = json!([
        { "toolName": "bash", "input": { "command": "ls" }, "content": ok, "isError": false },
        { "toolName": "bash", "input": { "command": "cargo build" }, "content": bad, "isError": true },
        { "toolName": "read", "input": { "path": "a" }, "content": ok, "isError": false },
    ]);

    let results = after_results(&w, &w.path("kelpie"), events);

    assert_eq!(results[0], serde_json::Value::Null);
    assert_eq!(results[1]["isError"], true);
    let content = results[1]["content"].as_array().unwrap();
    assert_eq!(content[0], bad[0]);
    assert!(
        content[1]["text"]
            .as_str()
            .unwrap()
            .contains("the build failed"),
        "{content:?}"
    );
    assert_eq!(results[2], serde_json::Value::Null);

    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 4, "{calls}");
    assert!(lines[0].starts_with("guard "), "{}", lines[0]);
    assert!(lines[0].ends_with("--after"), "{}", lines[0]);
    let second: serde_json::Value = serde_json::from_str(lines[3]).unwrap();
    assert_eq!(second["tool_name"], "Bash");
    assert_eq!(second["tool_input"]["command"], "cargo build");
    assert_eq!(second["tool_response"]["exit_code"], 101);
    assert!(
        second["tool_response"]["stdout"]
            .as_str()
            .unwrap()
            .starts_with("refuse-me"),
    );
}

#[test]
fn a_pi_write_hands_confine_the_content_for_the_projects_write_hooks() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    let log = w.path("checks.log");
    let script = format!(
        "#!/bin/sh\ncat >> {log}\necho >> {log}\n",
        log = log.display()
    );
    crate::test::write_script(&w.path("kelpie"), &script);
    let events = json!([
        { "toolName": "write", "input": { "path": "a.txt", "content": "one" } },
        { "toolName": "edit", "input": { "path": "b.txt", "edits": [
            { "oldText": "x", "newText": "y" }, { "oldText": "z", "newText": "w" } ] } },
    ]);

    guarded(&w, &w.path("kelpie"), events);

    let calls = std::fs::read_to_string(&log).unwrap();
    let seen: Vec<serde_json::Value> = calls
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(seen[0]["tool_input"]["content"], "one");
    assert_eq!(seen[1]["tool_input"]["content"], "y\nw");
}

#[test]
fn a_guard_whose_checks_cannot_run_refuses() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    let events = json!([
        { "toolName": "bash", "input": { "command": "ls" } },
        { "toolName": "write", "input": { "path": "a.txt", "content": "" } },
    ]);
    let verdicts = guarded(&w, &w.path("no-such-kelpie"), events);
    assert!(verdicts.iter().all(|v| v["block"] == true), "{verdicts:?}");
}

// Loads the guard extension as pi does and hands its `tool_result` handler two
// events, with a context that counts the aborts asked of it.
const ABORT_RUNNER: &str = r#"
import { writeFileSync } from "node:fs";
import guard from "./guard.mts";
let handler;
guard({ on: (name, fn) => { if (name === "tool_result") handler = fn; } });
let aborts = 0;
const ctx = { cwd: process.argv[2], abort: () => { aborts += 1; } };
const event = { toolName: "read", input: { path: "a" }, content: [], isError: false };
const first = (await handler(event, ctx)) ?? null;
process.stdout.write(JSON.stringify({ aborts_before: aborts }) + "\n");
writeFileSync(process.argv[3], "");
const second = (await handler(event, ctx)) ?? null;
console.log(JSON.stringify({ aborts, first, second }));
"#;

#[test]
fn the_guard_extension_aborts_after_a_tool_result_while_a_pause_is_set() {
    if !has_node() {
        eprintln!("skipped: needs node, which loads pi's extensions");
        return;
    }
    let w = World::new();
    let call = w.fenced(URL, Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let fence = call.reach.fence.as_deref().unwrap();
    let pause = crate::guard::Pause::in_build(&fence.guard.build);
    std::fs::create_dir_all(&fence.guard.build).unwrap();
    std::fs::write(w.path("guard.mts"), guard_extension(fence)).unwrap();
    std::fs::write(w.path("run.mjs"), ABORT_RUNNER).unwrap();
    // The runner sets the pause between its two results, as `drain pause` does mid-turn.
    let out = std::process::Command::new("node")
        .arg(w.path("run.mjs"))
        .arg(w.path("wt"))
        .arg(pause.requested_file())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    assert_eq!(
        lines[0]["aborts_before"], 0,
        "no pause is set: the turn goes on"
    );
    assert_eq!(lines[1]["aborts"], 1, "the next result ends the turn");
    assert_eq!(
        lines[1]["second"],
        serde_json::Value::Null,
        "the result is kept"
    );
    assert!(pause.ended_turn(), "the runner tells a paused turn by this");
}
