//! A goose worker's guard: what goose's `PreToolUse` payload becomes in the
//! guards Claude Code's calls already go through

use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::settings::{HookPoint, NonBlank, ProjectHook};

const HOME: &str = "/home/me";

// Refuses a payload naming the word, with a reason that does not repeat it.
const REFUSE_WORD: &str =
    "if grep -q Acme; then echo 'it names a word the project keeps private' >&2; exit 2; fi";

// A worker's worktree and build folder, both real folders.
struct Worker {
    worktree: TempDir,
    build: TempDir,
}

impl Worker {
    fn new() -> Self {
        let worktree = crate::test::short_tempdir();
        std::fs::create_dir(worktree.path().join("src")).unwrap();
        Self {
            worktree,
            build: crate::test::short_tempdir(),
        }
    }

    // goose's payload for `tool` with `input`, run in `working_dir`.
    fn payload(tool: &str, input: Value, working_dir: &Path) -> Value {
        json!({
            "event": "PreToolUse",
            "session_id": "20260101_1",
            "matcher_context": tool,
            "tool_name": tool,
            "tool_input": input,
            "working_dir": working_dir,
        })
    }

    fn judge_raw(&self, payload: &[u8], hooks: &Hooks) -> Verdict {
        self.judge_loaded(payload, hooks, &[])
    }

    // The verdict when the call loaded the MCP servers with these keys.
    fn judge_loaded(&self, payload: &[u8], hooks: &Hooks, servers: &[String]) -> Verdict {
        let checkout = Checkout {
            git_common_dir: Path::new("/nowhere"),
            worktree: self.worktree.path(),
            base: "main",
            kelpie_pushes: false,
        };
        judge_goose(
            payload,
            Some(Path::new(HOME)),
            LocalPaths::new([Path::new(HOME)]),
            hooks,
            checkout,
            self.build.path(),
            servers,
        )
    }

    fn judge_in(&self, tool: &str, input: Value, working_dir: &Path, hooks: &Hooks) -> Verdict {
        let payload = Self::payload(tool, input, working_dir);
        self.judge_raw(payload.to_string().as_bytes(), hooks)
    }

    fn judge(&self, tool: &str, input: Value) -> Verdict {
        self.judge_in(tool, input, self.worktree.path(), &Hooks::default())
    }

    // What the command guard says of `command` run in the worktree as Claude Code's `Bash`.
    fn claude_says(&self, command: &str) -> Verdict {
        let call = json!({
            "tool_name": "Bash",
            "cwd": self.worktree.path(),
            "tool_input": { "command": command },
        });
        let checkout = Checkout {
            git_common_dir: Path::new("/nowhere"),
            worktree: self.worktree.path(),
            base: "main",
            kelpie_pushes: false,
        };
        crate::guard::judge(
            call.to_string().as_bytes(),
            Some(Path::new(HOME)),
            LocalPaths::new([Path::new(HOME)]),
            checkout,
        )
    }
}

fn refusal(verdict: Verdict) -> String {
    match verdict {
        Verdict::Refuse(why) => why,
        Verdict::Allow => panic!("the call was let through"),
    }
}

fn hooks(list: &[(HookPoint, &str)]) -> Hooks {
    let list: Vec<ProjectHook> = list
        .iter()
        .map(|&(point, command)| ProjectHook {
            point,
            command: NonBlank::try_from(command.to_owned()).unwrap(),
        })
        .collect();
    Hooks::new("koji", "acme/koji", Path::new("/"), &list)
}

#[test]
fn a_shell_command_goes_to_the_command_guard_and_a_refused_one_exits_2_with_its_reason() {
    let w = Worker::new();

    assert_eq!(
        w.judge("shell", json!({ "command": "ls -la src" })),
        Verdict::Allow
    );
    // `main` turns a refusal into exit 2 with this text on stderr: the process
    // test in `tests/guard_goose.rs` reads both.
    let why = refusal(w.judge("shell", json!({ "command": "gh pr merge 1" })));
    assert!(!why.is_empty());
    assert_eq!(
        Verdict::Refuse(why),
        w.claude_says("gh pr merge 1"),
        "the reason is the one Claude Code's `Bash` gets"
    );
    // goose's optional `timeout_secs` changes nothing.
    refusal(w.judge(
        "shell",
        json!({ "command": "gh pr merge 1", "timeout_secs": 5 }),
    ));
}

#[test]
fn a_write_and_an_edit_go_to_confine_with_their_text_and_a_path_resolved_from_the_working_folder() {
    let w = Worker::new();
    let tree = w.worktree.path();
    let all = Hooks::default();

    // A relative path is the working folder's, whichever folder that is.
    let write = json!({ "path": "a.rs", "content": "fn main() {}" });
    assert_eq!(
        w.judge_in("write", write.clone(), &tree.join("src"), &all),
        Verdict::Allow
    );
    let edit = json!({ "path": "a.rs", "before": "x", "after": "y" });
    assert_eq!(
        w.judge_in("edit", edit.clone(), &tree.join("src"), &all),
        Verdict::Allow
    );
    // One folder up, the same relative path leaves the worktree.
    let up = json!({ "path": "../outside.txt", "content": "x" });
    refusal(w.judge_in("write", up, &tree.join("src"), &all));
    // A path in the build folder is the worker's to write.
    let build = json!({ "path": w.build.path().join("notes.md"), "content": "x" });
    assert_eq!(w.judge("write", build), Verdict::Allow);
    // And a harness's own folder inside the worktree is not, for either tool.
    let settings = json!({ "path": ".claude/settings.json", "content": "{}" });
    refusal(w.judge("write", settings));
    let goose = json!({ "path": ".config/goose/settings.json", "before": "", "after": "{}" });
    refusal(w.judge("edit", goose));

    // The text rides along for the project's `write` hooks: a write's
    // `content` and an edit's `after`.
    let keep = hooks(&[(HookPoint::Write, REFUSE_WORD)]);
    let why = refusal(w.judge_in(
        "write",
        json!({ "path": "src/a.rs", "content": "// Acme" }),
        tree,
        &keep,
    ));
    assert!(why.contains("a word the project keeps private"), "{why}");
    refusal(w.judge_in(
        "edit",
        json!({ "path": "src/a.rs", "before": "x", "after": "Acme" }),
        tree,
        &keep,
    ));
    // Text in `before` is the file's own, which the hook is not asked about.
    assert_eq!(
        w.judge_in(
            "edit",
            json!({ "path": "src/a.rs", "before": "Acme", "after": "ok" }),
            tree,
            &keep
        ),
        Verdict::Allow
    );
}

#[test]
fn a_tree_is_allowed_and_any_other_tool_is_refused_and_so_is_an_unreadable_payload() {
    let w = Worker::new();

    assert_eq!(
        w.judge("tree", json!({ "path": ".", "depth": 2 })),
        Verdict::Allow
    );
    // A tree with no path lists the working folder.
    assert_eq!(w.judge("tree", json!({})), Verdict::Allow);
    // The skills extension's tool reads inside goose.
    assert_eq!(
        w.judge("load_skill", json!({ "name": "tdd" })),
        Verdict::Allow
    );
    // Tools goose offers a worker that kelpie does not allow, and names it has never heard of.
    for tool in [
        "read_image",
        "delegate",
        "load",
        "developer__shell",
        "todo__todo_write",
        "Bash",
        "",
    ] {
        let why = refusal(w.judge(tool, json!({})));
        assert!(why.contains("only these tools"), "{tool}: {why}");
    }
    let long = "x".repeat(500);
    let why = refusal(w.judge(&long, json!({})));
    assert!(why.len() < 400, "a long name is not quoted whole: {why}");

    // Whatever cannot be read refuses, never allows.
    let tree = w.worktree.path();
    let unreadable: [&[u8]; 3] = [b"", b"not json", b"{}"];
    for payload in unreadable {
        refusal(w.judge_raw(payload, &Hooks::default()));
    }
    let nameless = json!({ "tool_input": {}, "working_dir": tree });
    refusal(w.judge_raw(nameless.to_string().as_bytes(), &Hooks::default()));
    let nowhere = json!({ "tool_name": "shell", "tool_input": { "command": "ls" } });
    refusal(w.judge_raw(nowhere.to_string().as_bytes(), &Hooks::default()));
    // A shell with no command and a file tool with no path or text have nothing to judge.
    refusal(w.judge("tree", json!({ "path": 7 })));
    refusal(w.judge("shell", json!({})));
    refusal(w.judge("shell", json!({ "command": 7 })));
    refusal(w.judge("write", json!({ "content": "x" })));
    refusal(w.judge("write", json!({ "path": "a.rs" })));
    refusal(w.judge("edit", json!({ "path": "a.rs", "before": "x" })));
    refusal(w.judge("edit", json!({ "path": 7, "after": "x" })));
}

#[test]
fn the_projects_command_and_write_hooks_judge_what_goose_runs() {
    let w = Worker::new();
    let tree = w.worktree.path();
    let keep = hooks(&[
        (HookPoint::Command, REFUSE_WORD),
        (HookPoint::Write, REFUSE_WORD),
    ]);

    let why = refusal(w.judge_in("shell", json!({ "command": "echo Acme" }), tree, &keep));
    assert!(
        why.contains("this command: the `command` hook `if` refused: it names a word"),
        "{why}"
    );
    let why = refusal(w.judge_in(
        "write",
        json!({ "path": "notes.md", "content": "Acme" }),
        tree,
        &keep,
    ));
    assert!(
        why.contains("this write to `notes.md`: the `write` hook `if` refused"),
        "{why}"
    );
    // What the hooks accept goes through, and a tree is no command or write.
    assert_eq!(
        w.judge_in("shell", json!({ "command": "echo fine" }), tree, &keep),
        Verdict::Allow
    );
    assert_eq!(
        w.judge_in(
            "write",
            json!({ "path": "notes.md", "content": "fine" }),
            tree,
            &keep
        ),
        Verdict::Allow
    );
    assert_eq!(
        w.judge_in("tree", json!({ "path": "." }), tree, &keep),
        Verdict::Allow
    );
    // A tree is the `ls` of its folder, so the command hooks judge it too.
    let why = refusal(w.judge_in("tree", json!({ "path": "Acme" }), tree, &keep));
    assert!(why.contains("the `command` hook `if` refused"), "{why}");
    // And a folder name does not run as a command.
    assert_eq!(
        w.judge_in("tree", json!({ "path": "a; gh pr merge 1" }), tree, &keep),
        Verdict::Allow
    );
}

#[test]
fn a_tool_of_a_loaded_server_goes_through_and_a_look_alike_does_not() {
    let w = Worker::new();
    let keys = ["search".to_owned(), "codebase-memory-mcp".to_owned()];
    let judge = |tool: &str| {
        let payload = Worker::payload(tool, json!({}), w.worktree.path());
        w.judge_loaded(payload.to_string().as_bytes(), &Hooks::default(), &keys)
    };
    assert_eq!(judge("search__find"), Verdict::Allow);
    assert_eq!(judge("codebase-memory-mcp__search_graph"), Verdict::Allow);
    for tool in [
        "search",
        "search__",
        "searcher__find",
        "Search__find",
        "other__find",
    ] {
        assert!(matches!(judge(tool), Verdict::Refuse(_)), "{tool}");
    }
    // With no server loaded the same name is refused.
    let payload = Worker::payload("search__find", json!({}), w.worktree.path());
    let verdict = w.judge_raw(payload.to_string().as_bytes(), &Hooks::default());
    assert!(matches!(verdict, Verdict::Refuse(_)));
}

#[test]
fn the_server_flags_are_taken_out_of_the_arguments() {
    let args = ["--goose=/b", "--goose-server=a", "--goose-server=b_c"].map(String::from);
    let (servers, rest) = goose_server_flags(&args);
    assert_eq!(servers, ["a", "b_c"]);
    assert_eq!(rest, ["--goose=/b"]);
}

#[test]
fn the_read_only_flag_allows_a_listed_command_and_refuses_write_edit_and_any_other_tool() {
    let w = Worker::new();
    let tree = w.worktree.path();
    let judge = |tool: &str, input: Value| {
        let payload = Worker::payload(tool, input, tree);
        judge_goose_reading(payload.to_string().as_bytes(), &Hooks::default(), &[])
    };

    // A listed command runs, and a pipe between listed commands.
    assert_eq!(
        judge("shell", json!({ "command": "cat src/lib.rs" })),
        Verdict::Allow
    );
    assert_eq!(
        judge("shell", json!({ "command": "rg todo src | wc -l" })),
        Verdict::Allow
    );
    // The read-only guard's own refusals reach goose's shell: a command off the
    // list, a redirect, git.
    for command in ["rm -rf src", "echo x > a.rs", "git log", "gh pr merge 1"] {
        let why = refusal(judge("shell", json!({ "command": command })));
        assert!(why.contains("plain words"), "{command}: {why}");
    }
    refusal(judge("shell", json!({})));
    refusal(judge("shell", json!({ "command": 7 })));

    assert_eq!(
        judge("load_skill", json!({ "name": "tdd" })),
        Verdict::Allow
    );

    // A tree is allowed, and it is the `ls` of its folder.
    assert_eq!(judge("tree", json!({ "path": "src" })), Verdict::Allow);
    assert_eq!(judge("tree", json!({})), Verdict::Allow);
    refusal(judge("tree", json!({ "path": 7 })));

    // `write`, `edit` and every other tool name are refused for the same reason.
    for (tool, input) in [
        ("write", json!({ "path": "a.rs", "content": "x" })),
        (
            "edit",
            json!({ "path": "a.rs", "before": "x", "after": "y" }),
        ),
        ("write", json!({})),
        ("read_image", json!({})),
        ("delegate", json!({})),
        ("Bash", json!({ "command": "ls" })),
        ("", json!({})),
    ] {
        let why = refusal(judge(tool, input));
        assert!(why.contains("writes no file"), "{tool}: {why}");
    }
    let long = "x".repeat(500);
    let why = refusal(judge(&long, json!({})));
    assert!(why.len() < 400, "a long name is not quoted whole: {why}");

    // Whatever cannot be read refuses.
    let unreadable: [&[u8]; 3] = [b"", b"not json", b"{}"];
    for payload in unreadable {
        refusal(judge_goose_reading(payload, &Hooks::default(), &[]));
    }

    // The project's `command` hooks judge what the list lets through.
    let keep = hooks(&[(HookPoint::Command, REFUSE_WORD)]);
    let named = Worker::payload("shell", json!({ "command": "cat Acme" }), tree);
    let why = refusal(judge_goose_reading(
        named.to_string().as_bytes(),
        &keep,
        &[],
    ));
    assert!(why.contains("the `command` hook `if` refused"), "{why}");
    let named = Worker::payload("tree", json!({ "path": "Acme" }), tree);
    let why = refusal(judge_goose_reading(
        named.to_string().as_bytes(),
        &keep,
        &[],
    ));
    assert!(why.contains("the `command` hook `if` refused"), "{why}");
}

#[test]
fn the_read_only_guard_lets_a_loaded_server_s_tools_through_and_refuses_the_rest() {
    let w = Worker::new();
    let tree = w.worktree.path();
    let servers = ["codebase-memory-mcp".to_owned()];
    let judge = |tool: &str| {
        let payload = Worker::payload(tool, json!({}), tree);
        judge_goose_reading(payload.to_string().as_bytes(), &Hooks::default(), &servers)
    };
    assert_eq!(judge("codebase-memory-mcp__search_graph"), Verdict::Allow);
    for tool in [
        "codebase-memory-mcp_x",
        "write",
        "edit",
        "other__search_graph",
    ] {
        refusal(judge(tool));
    }
}
