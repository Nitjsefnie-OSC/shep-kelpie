//! `kelpie guard --goose=<build folder>`, as a goose worker's `PreToolUse`
//! hook runs it, against the real binary: a refused call exits 2 with its
//! reason on stderr and nothing on stdout, and an allowed one exits 0 with
//! nothing on either.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const KELPIE: &str = env!("CARGO_BIN_EXE_shep-kelpie");

/// A worker's worktree and build folder
struct Worker {
    _dir: tempfile::TempDir,
    root: PathBuf,
    worktree: PathBuf,
    build: PathBuf,
}

impl Worker {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let worktree = root.join("wt");
        let build = root.join("build");
        std::fs::create_dir_all(worktree.join("src")).unwrap();
        std::fs::create_dir_all(&build).unwrap();
        Self {
            _dir: dir,
            root,
            worktree,
            build,
        }
    }

    // What the hook does with goose's payload for `tool`, as goose runs it.
    fn hook(&self, tool: &str, input: Value) -> Output {
        self.hook_with(tool, input, &[])
    }

    // The same with `flags` after the build folder: `--read-only` is a
    // reviewer's, advisor's and retro's.
    fn hook_with(&self, tool: &str, input: Value, flags: &[&str]) -> Output {
        let mut hook = Command::new(KELPIE)
            .arg("guard")
            .arg(self.root.join("repo.git"))
            .arg(&self.worktree)
            .arg(format!("--goose={}", self.build.display()))
            .args(flags)
            .env("HOME", self.root.join("home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = json!({
            "event": "PreToolUse",
            "session_id": "20260101_1",
            "matcher_context": tool,
            "tool_name": tool,
            "tool_input": input,
            "working_dir": self.worktree,
        });
        hook.stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        hook.wait_with_output().unwrap()
    }
}

fn refusal(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    let why = String::from_utf8(out.stderr.clone()).unwrap();
    assert!(!why.trim().is_empty(), "a refusal gives its reason");
    why
}

fn allowed(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
}

#[test]
fn a_write_into_a_harness_folder_exits_2_with_its_reason() {
    let w = Worker::new();

    let out = w.hook(
        "write",
        json!({ "path": ".claude/settings.json", "content": "{}" }),
    );
    refusal(&out);
    // The same write to an ordinary file, and into the build folder, goes through.
    allowed(&w.hook(
        "write",
        json!({ "path": "src/a.rs", "content": "fn a() {}" }),
    ));
    let notes = w.build.join("notes.md");
    allowed(&w.hook("write", json!({ "path": notes, "content": "x" })));
}

#[test]
fn a_command_the_guard_refuses_exits_2_with_its_reason() {
    let w = Worker::new();

    let why = refusal(&w.hook("shell", json!({ "command": "gh pr merge 1" })));
    assert!(why.contains("merge"), "{why}");
    allowed(&w.hook("shell", json!({ "command": "ls src" })));
}

#[test]
fn a_tool_kelpie_does_not_allow_and_an_unreadable_payload_exit_2() {
    let w = Worker::new();

    let why = refusal(&w.hook("read_image", json!({ "path": "a.png" })));
    assert!(why.contains("only these tools"), "{why}");
    allowed(&w.hook("tree", json!({ "path": "." })));

    let mut hook = Command::new(KELPIE)
        .arg("guard")
        .arg(w.root.join("repo.git"))
        .arg(&w.worktree)
        .arg(format!("--goose={}", w.build.display()))
        .env("HOME", w.root.join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin.take().unwrap().write_all(b"not json").unwrap();
    refusal(&hook.wait_with_output().unwrap());
}

#[test]
fn with_read_only_a_listed_command_exits_0_and_a_write_exits_2() {
    let w = Worker::new();
    let read = |tool: &str, input: Value| w.hook_with(tool, input, &["--read-only"]);

    allowed(&read("shell", json!({ "command": "ls src" })));
    allowed(&read("tree", json!({ "path": "." })));
    let why = refusal(&read("shell", json!({ "command": "rm -r src" })));
    assert!(why.contains("plain words"), "{why}");
    let why = refusal(&read(
        "write",
        json!({ "path": "src/a.rs", "content": "fn a() {}" }),
    ));
    assert!(why.contains("writes no file"), "{why}");
}
