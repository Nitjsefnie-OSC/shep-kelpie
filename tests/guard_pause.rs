//! `kelpie guard --pause=<build>`, as a worker's `PostToolUse` hook runs it,
//! against the real binary: it ends the turn only while a pause is set.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

const KELPIE: &str = env!("CARGO_BIN_EXE_shep-kelpie");

// What the hook prints for a finished tool call, with `build` as the pause's folder.
fn after_tool(root: &Path, build: &Path) -> String {
    let mut hook = Command::new(KELPIE)
        .arg("guard")
        .arg(root.join("repo.git"))
        .arg(root.join("wt"))
        .arg(format!("--pause={}", build.display()))
        .env("HOME", root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let call = json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "cwd": root.join("wt"),
        "tool_input": { "command": "true" },
        "tool_response": { "stdout": "" },
    });
    hook.stdin
        .take()
        .unwrap()
        .write_all(call.to_string().as_bytes())
        .unwrap();
    let out = hook.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_hook_answers_nothing_until_a_pause_is_set_and_then_ends_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let build = root.join("build");
    std::fs::create_dir_all(&build).unwrap();

    assert_eq!(after_tool(&root, &build), "", "no pause is set");

    std::fs::write(build.join("kelpie-pause"), "").unwrap();
    let answer: Value = serde_json::from_str(&after_tool(&root, &build)).unwrap();
    assert_eq!(answer["continue"], false);
    assert!(
        build.join("kelpie-paused").exists(),
        "the hook says the turn ended for the pause"
    );
}
