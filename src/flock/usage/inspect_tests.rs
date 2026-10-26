//! `usage --item`, `--context` and `--reads`, over a ledger and transcripts
//! in a temp home, found as a runner's steps file places them

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use crate::usage::{Line, append_to};

const AT: u64 = 1_791_000_000;

struct Rig {
    home: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("koji")).unwrap();
        Self { home }
    }

    fn ask(&self, args: &[&str]) -> Result<Vec<String>, String> {
        usage(self.home.path(), None, args)
    }

    // A call of the ledger, ended `after` seconds into the day
    fn call(&self, issue: Option<u64>, role: &str, harness: &str, session: &str, after: u64) {
        let line = json!({
            "line": "call", "at": AT + after, "issue": issue, "pull_request": issue.map(|_| 12),
            "role": role, "kind": "turn", "harness": harness, "model": "claude-sonnet-5-5",
            "session": session,
            "usage": {"input": 10, "cache_write": 1000, "cache_read": 20000, "output": 300},
            "units": 5_000, "cost_usd": 0.5, "seconds": 90, "ended": "answered",
        });
        let line: Line = serde_json::from_value(line).unwrap();
        append_to(
            &self.home.path().join("koji").join(crate::usage::FILE),
            &line,
        )
        .unwrap();
    }

    // The transcript of `session`, written where its harness would, and the
    // `session-started` step that names it
    fn transcript(&self, harness: &str, session: &str, lines: &[Value]) -> PathBuf {
        let (folder, file) = match harness {
            "pi" => (
                self.home.path().join(".pi/sessions/-tmp-koji"),
                format!("2026-10-03T04-00-00-000Z_{session}.jsonl"),
            ),
            _ => (
                self.home.path().join(".claude/projects/-tmp-koji"),
                format!("{session}.jsonl"),
            ),
        };
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(file);
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&path, text).unwrap();
        self.started(harness, session, &folder);
        path
    }

    fn started(&self, harness: &str, session: &str, folder: &Path) {
        let step = json!({
            "at": "2026-10-03T04:00:00.000Z", "step": "session-started", "issue": 7,
            "role": "worker", "agent": "claude", "harness": harness, "session": session,
            "transcript": {"folder": folder},
        });
        let steps = self.home.path().join("koji").join(crate::home::STEPS_FILE);
        let mut text = std::fs::read_to_string(&steps).unwrap_or_default();
        text.push_str(&format!("{step}\n"));
        std::fs::write(steps, text).unwrap();
    }
}

// One request of Claude Code's: a line a content block, all with the
// request's id and usage
fn claude_request(id: &str, usage: [u64; 4], blocks: &[Value]) -> Vec<Value> {
    let [input, cache_write, cache_read, output] = usage;
    blocks
        .iter()
        .map(|block| {
            json!({
                "type": "assistant", "isSidechain": false,
                "timestamp": "2026-10-03T04:00:05.000Z",
                "message": {
                    "id": id, "model": "claude-sonnet-5-5", "role": "assistant",
                    "content": [block],
                    "usage": {
                        "input_tokens": input, "cache_creation_input_tokens": cache_write,
                        "cache_read_input_tokens": cache_read, "output_tokens": output,
                    },
                },
            })
        })
        .collect()
}

fn tool(name: &str, path: &str) -> Value {
    json!({"type": "tool_use", "id": "toolu_1", "name": name, "input": {"file_path": path}})
}

fn thought() -> Value {
    json!({"type": "thinking", "thinking": "", "signature": "x"})
}

// A worker session of three requests, the first a thought and a read
fn worker_lines() -> Vec<Value> {
    let mut lines = claude_request(
        "msg_1",
        [10, 9_000, 0, 100],
        &[thought(), tool("Read", "src/a.rs")],
    );
    lines.push(json!({"type": "user", "message": {"role": "user", "content": []}}));
    lines.extend(claude_request(
        "msg_2",
        [10, 1_000, 9_000, 200],
        &[tool("Read", "src/b.rs")],
    ));
    lines.extend(claude_request(
        "msg_3",
        [10, 500, 10_000, 300],
        &[tool("Edit", "src/a.rs")],
    ));
    lines
}

#[test]
fn item_lists_the_work_items_calls_in_order_with_their_transcripts() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    rig.call(Some(8), "worker", "claude-code", "s-other", 10);
    rig.call(Some(7), "reviewer", "claude-code", "s-review", 100);
    let file = rig.transcript("claude-code", "s-worker", &worker_lines());

    let out = rig.ask(&["koji", "--item", "7"]).unwrap();

    assert_eq!(
        out[0],
        "koji #7 (pull request #12): 2 calls, 10,000 units, $1.00"
    );
    let worker = out.iter().position(|l| l.contains("worker")).unwrap();
    let reviewer = out.iter().position(|l| l.contains("reviewer")).unwrap();
    assert!(worker < reviewer, "{out:#?}");
    let cells: Vec<&str> = out[worker].split_whitespace().collect();
    assert_eq!(
        cells[1..],
        [
            "worker",
            "turn",
            "claude-code",
            "claude-sonnet-5-5",
            "10",
            "1,000",
            "20,000",
            "300",
            "5,000",
            "$0.50",
            "90",
            "answered"
        ]
    );
    assert!(!out.iter().any(|l| l.contains("s-other")), "{out:#?}");
    let found = format!(
        "  s-worker worker (claude-code): {}, 3 requests",
        file.display()
    );
    assert!(out.contains(&found), "{out:#?}");
    assert!(
        out.contains(&"  s-review reviewer (claude-code): no transcript found".to_owned()),
        "{out:#?}"
    );
}

#[test]
fn item_lists_calls_by_when_they_ended_not_where_an_import_put_them() {
    let rig = Rig::new();
    rig.call(Some(7), "reviewer", "claude-code", "s-late", 600);
    rig.call(Some(7), "worker", "pi", "s-late", 300);

    let out = rig.ask(&["koji", "--item", "7", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    assert_eq!(json["calls"][0]["role"], "worker");
    assert_eq!(json["calls"][1]["role"], "reviewer");
    assert_eq!(json["sessions"][0]["role"], "worker");
    assert_eq!(json["sessions"][0]["harness"], "pi");
}

#[test]
fn item_as_json_holds_the_calls_and_the_sessions() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    let file = rig.transcript("claude-code", "s-worker", &worker_lines());

    let out = rig.ask(&["koji", "--item", "7", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    assert_eq!(json["issue"], "7");
    assert_eq!(json["pull_request"], 12);
    assert_eq!(json["calls"][0]["role"], "worker");
    assert_eq!(json["calls"][0]["usage"]["cache_read"], 20_000);
    assert_eq!(json["sessions"][0]["session"], "s-worker");
    assert_eq!(
        json["sessions"][0]["transcript"],
        file.display().to_string()
    );
    assert_eq!(json["sessions"][0]["requests"], 3);
}

#[test]
fn item_with_no_calls_says_so() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    assert_eq!(
        rig.ask(&["koji", "--item", "9"]).unwrap(),
        ["koji: no calls recorded for #9"]
    );
}

#[test]
fn context_sizes_a_requests_once_however_many_lines_it_wrote() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    rig.transcript("claude-code", "s-worker", &worker_lines());

    let out = rig.ask(&["koji", "--context"]).unwrap();

    let row: Vec<&str> = out
        .iter()
        .find(|l| l.starts_with("worker"))
        .unwrap()
        .split_whitespace()
        .collect();
    // 4 lines of 3 requests, whose contexts are 9,010, 10,010 and 10,510
    assert_eq!(
        row,
        [
            "worker",
            "claude-code",
            "1",
            "3",
            "9,010",
            "10,010",
            "10,510",
            "63%",
            "35%"
        ]
    );
}

#[test]
fn context_leaves_out_a_subagents_lines_and_reads_pi() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    rig.call(Some(7), "reviewer", "pi", "s-pi", 5);
    let mut lines = worker_lines();
    lines.extend(
        claude_request("msg_side", [1, 900_000, 0, 1], &[thought()])
            .into_iter()
            .map(|mut line| {
                line["isSidechain"] = json!(true);
                line
            }),
    );
    rig.transcript("claude-code", "s-worker", &lines);
    let pi = |id: &str, usage: Value| {
        json!({
            "type": "message", "id": id, "timestamp": "2026-10-03T04:00:05.000Z",
            "message": {"role": "assistant", "content": [], "usage": usage},
        })
    };
    rig.transcript(
        "pi",
        "s-pi",
        &[
            json!({"type": "session", "id": "s-pi"}),
            pi(
                "a",
                json!({"input": 100, "output": 5, "cacheRead": 0, "cacheWrite": 2000}),
            ),
            pi(
                "b",
                json!({"input": 100, "output": 5, "cacheRead": 2000, "cacheWrite": 300}),
            ),
        ],
    );

    let out = rig.ask(&["koji", "--context", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let groups = json["groups"].as_array().unwrap();
    let worker = &groups[1];
    assert_eq!(
        (worker["role"].as_str(), worker["requests"].as_u64()),
        (Some("worker"), Some(3))
    );
    assert_eq!(worker["largest_request"], 10_510);
    let reviewer = &groups[0];
    assert_eq!(reviewer["harness"], "pi");
    assert_eq!(reviewer["requests"], 2);
    assert_eq!(reviewer["first_request"], 2_100);
    assert_eq!(reviewer["largest_request"], 2_400);
    assert_eq!(reviewer["median_request"], 2_250);
}

#[test]
fn context_counts_a_session_with_no_transcript_apart_and_honours_since() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-worker", 0);
    rig.call(Some(7), "worker", "claude-code", "s-lost", 5);
    rig.transcript("claude-code", "s-worker", &worker_lines());

    let out = rig.ask(&["koji", "--context"]).unwrap();
    assert!(
        out.last()
            .unwrap()
            .starts_with("1 of 2 sessions are left out"),
        "{out:#?}"
    );

    let out = rig
        .ask(&["koji", "--context", "--since", "2030-01-01"])
        .unwrap();
    assert_eq!(out, ["koji: no sessions recorded in the ledger"]);
}

#[test]
fn reads_split_what_a_worker_read_before_its_first_edit() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-edit", 0);
    rig.call(Some(8), "worker", "claude-code", "s-none", 5);
    rig.call(Some(7), "reviewer", "claude-code", "s-review", 6);
    let mut lines = Vec::new();
    for (n, block) in [
        tool("Read", "src/usage/report.rs"),
        tool("Read", "src/usage/item.rs"),
        tool("Read", "src/usage.rs"),
        tool("Read", "docs/design/metrics.md"),
        tool("Read", "src/watch/log.rs"),
        tool("Read", "src/watch/log.rs"),
        tool("Edit", "src/usage/report.rs"),
        tool("Edit", "src/usage.rs"),
        tool("Read", "src/other.rs"),
    ]
    .into_iter()
    .enumerate()
    {
        lines.extend(claude_request(&format!("m{n}"), [1, 1, 1, 1], &[block]));
    }
    rig.transcript("claude-code", "s-edit", &lines);
    let reads_only = claude_request("m", [1, 1, 1, 1], &[tool("Read", "src/a.rs")]);
    rig.transcript("claude-code", "s-none", &reads_only);
    rig.transcript("claude-code", "s-review", &lines);

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let group = &json["groups"][0];
    assert_eq!(json["groups"].as_array().unwrap().len(), 1, "workers only");
    assert_eq!(group["harness"], "claude-code");
    assert_eq!(
        (group["sessions"].as_u64(), group["no_edit"].as_u64()),
        (Some(1), Some(1))
    );
    // report.rs and usage.rs were edited; item.rs sits beside report.rs; the
    // doc is a doc; log.rs, read twice, is one other file.
    assert_eq!(group["edited"], 2);
    assert_eq!(group["neighbours"], 1);
    assert_eq!(group["docs"], 1);
    assert_eq!(group["other_code"], 1);
    assert_eq!(group["median_files"], 5);

    let out = rig.ask(&["koji", "--reads"]).unwrap();
    let row: Vec<&str> = out[2].split_whitespace().collect();
    assert_eq!(
        row,
        [
            "claude-code",
            "1",
            "5",
            "2",
            "(40%)",
            "1",
            "(20%)",
            "1",
            "(20%)",
            "1",
            "(20%)",
            "5"
        ]
    );
    assert_eq!(out[3], "1 sessions edited no file, and are left out");
}

#[test]
fn reads_count_one_file_spelt_relative_and_absolute_as_one() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-paths", 0);
    let mut lines = Vec::new();
    for (n, block) in [
        tool("Read", "./src/a.rs"),
        tool("Read", "/repo/src/../src/b.rs"),
        tool("Read", "/repo/docs//guide.md"),
        tool("Edit", "src/a.rs"),
        tool("Edit", "/repo/src/b.rs"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = claude_request(&format!("m{n}"), [1, 1, 1, 1], &[block]);
        for line in &mut request {
            line["cwd"] = json!("/repo");
        }
        lines.extend(request);
    }
    rig.transcript("claude-code", "s-paths", &lines);

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let group = &json["groups"][0];
    assert_eq!(group["edited"], 2);
    assert_eq!(group["docs"], 1);
    assert_eq!(group["neighbours"], 0);
    assert_eq!(group["other_code"], 0);
}

#[test]
fn a_docs_folder_above_the_checkout_does_not_make_every_read_a_doc() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-docs", 0);
    let mut lines = Vec::new();
    for (n, block) in [
        tool("Read", "src/a.rs"),
        tool("Read", "docs/guide.md"),
        tool("Edit", "lib/b.rs"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = claude_request(&format!("m{n}"), [1, 1, 1, 1], &[block]);
        for line in &mut request {
            line["cwd"] = json!("/Users/me/docs/proj");
        }
        lines.extend(request);
    }
    rig.transcript("claude-code", "s-docs", &lines);

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let group = &json["groups"][0];
    assert_eq!(group["docs"], 1);
    assert_eq!(group["other_code"], 1);
}

#[test]
fn context_says_a_transcript_with_no_request_was_read() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "claude-code", "s-cut", 0);
    let mut cut = claude_request("m", [0, 0, 0, 0], &[thought()]);
    cut[0]["message"]["model"] = json!("<synthetic>");
    rig.transcript("claude-code", "s-cut", &cut);

    let out = rig.ask(&["koji", "--context"]).unwrap();
    let json: Value = serde_json::from_str(
        &rig.ask(&["koji", "--context", "--json"])
            .unwrap()
            .join("\n"),
    )
    .unwrap();

    assert!(
        out.iter()
            .any(|l| l
                == "1 of 1 sessions are left out: their transcript was read and holds no request"),
        "{out:#?}"
    );
    assert!(
        !out.iter().any(|l| l.contains("no transcript was found")),
        "{out:#?}"
    );
    assert_eq!(
        (
            json["groups"][0]["unread"].as_u64(),
            json["groups"][0]["empty"].as_u64()
        ),
        (Some(0), Some(1))
    );
}

#[test]
fn only_the_sessions_asked_for_are_looked_for() {
    let rig = Rig::new();
    let lines = claude_request("m", [1, 1, 1, 1], &[thought()]);
    let wanted = rig.transcript("claude-code", "s-wanted", &lines);
    rig.transcript("claude-code", "s-other", &lines);
    let steps = std::fs::read_to_string(rig.home.path().join("koji").join(crate::home::STEPS_FILE))
        .unwrap();

    let wanted_only = std::collections::BTreeSet::from(["s-wanted".to_owned()]);
    let found = crate::watch::transcripts_of(&steps, &wanted_only);

    assert_eq!(
        found.keys().map(String::as_str).collect::<Vec<_>>(),
        ["s-wanted"]
    );
    assert_eq!(found["s-wanted"].1, wanted);
}

#[test]
fn reads_reads_pis_session_files() {
    let rig = Rig::new();
    rig.call(Some(7), "worker", "pi", "s-pi", 0);
    let call = |name: &str, path: &str| json!({"type": "toolCall", "id": "c", "name": name, "arguments": {"path": path}});
    let message = |id: &str, block: Value| {
        json!({
            "type": "message", "id": id, "timestamp": "2026-10-03T04:00:05.000Z",
            "message": {
                "role": "assistant", "content": [block],
                "usage": {"input": 5, "output": 5, "cacheRead": 0, "cacheWrite": 0},
            },
        })
    };
    rig.transcript(
        "pi",
        "s-pi",
        &[
            json!({"type": "session", "id": "s-pi"}),
            message("a", call("read", "@src/a.rs")),
            message("b", call("read", "README.md")),
            message("c", call("write", "src/a.rs")),
        ],
    );

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let group = &json["groups"][0];
    assert_eq!(
        (group["harness"].as_str(), group["edited"].as_u64()),
        (Some("pi"), Some(1))
    );
    assert_eq!(group["docs"], 1);
}

// A Codex 0.159.3 session that compacted: five requests, the fourth the compaction's own.
fn codex_rig(session: &str) -> Rig {
    let rig = Rig::new();
    rig.call(Some(1138), "worker", "codex", session, 0);
    let folder = rig.home.path().join(".codex/sessions");
    std::fs::create_dir_all(folder.join("2026/10/21")).unwrap();
    let recorded = include_str!("../../../fixtures/codex-rollout-compacted.jsonl");
    std::fs::write(
        folder.join("2026/10/21/rollout-2026-10-21T15-40-45-01a0f7fd.jsonl"),
        recorded,
    )
    .unwrap();
    rig.started("codex", session, &folder);
    rig
}

#[test]
fn a_recorded_codex_rollout_reads_as_its_five_requests() {
    let rig = codex_rig("s-codex");

    let out = rig.ask(&["koji", "--context"]).unwrap();
    let row: Vec<&str> = out[2].split_whitespace().collect();
    assert_eq!(
        row,
        [
            "worker", "codex", "1", "5", "19,244", "19,478", "29,387", "68%", "0%"
        ],
        "the cached tokens are 79,104 of 116,705"
    );

    let out = rig.ask(&["koji", "--item", "1138"]).unwrap();
    assert!(
        out.last().unwrap().ends_with(".jsonl, 5 requests"),
        "{out:#?}"
    );
}

#[test]
fn a_codex_rollout_shows_no_file_a_worker_read_or_edited() {
    let rig = codex_rig("s-codex");

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();

    let group = &json["groups"][0];
    assert_eq!(group["harness"], "codex");
    assert_eq!(
        (group["sessions"].as_u64(), group["no_edit"].as_u64()),
        (Some(0), Some(1)),
        "a rollout names no file, so the session edited none"
    );
    assert_eq!(group["unread"], 0);
}

#[test]
fn the_new_reports_refuse_what_they_cannot_take() {
    let rig = Rig::new();
    let err = |args: &[&str]| rig.ask(args).unwrap_err();
    assert!(err(&["--item", "7"]).contains("takes the project"));
    assert!(err(&["--context"]).contains("takes the project"));
    assert!(err(&["koji", "--item", "seven"]).contains("is not a ticket id"));
    assert!(err(&["koji", "--item", "0"]).contains("is not a ticket id"));
    // One report at a time, and `--since` means nothing to an item.
    assert!(err(&["koji", "--context", "--reads"]).contains("shep kelpie usage"));
    assert!(err(&["koji", "--item", "7", "--since", "2026-10-01"]).contains("shep kelpie usage"));
    // The ledger summary and the forge report have no JSON form.
    assert!(err(&["koji", "--json"]).contains("shep kelpie usage"));
    assert!(err(&["--forge", "--json"]).contains("shep kelpie usage"));
}

// A worker's own transcript, trimmed to 25 lines: seven requests whose blocks
// Claude Code wrote a line each, two reads and the edits after them.
#[test]
fn a_recorded_claude_code_transcript_reads_as_its_seven_requests() {
    let rig = Rig::new();
    let session = "7d1c4e9a-2b3f-4a60-9c85-1e0f6a3b8d52";
    rig.call(Some(926), "worker", "claude-code", session, 0);
    let folder = rig
        .home
        .path()
        .join(".claude/projects/-Users-me-shep-kelpie");
    std::fs::create_dir_all(&folder).unwrap();
    let recorded = include_str!("../../../fixtures/claude-transcript-requests.jsonl");
    std::fs::write(folder.join(format!("{session}.jsonl")), recorded).unwrap();
    rig.started("claude-code", session, &folder);

    let out = rig.ask(&["koji", "--context"]).unwrap();
    let row: Vec<&str> = out[2].split_whitespace().collect();
    assert_eq!(
        row,
        [
            "worker",
            "claude-code",
            "1",
            "7",
            "135,312",
            "142,597",
            "144,162",
            "99%",
            "1%"
        ]
    );

    let out = rig.ask(&["koji", "--reads", "--json"]).unwrap();
    let json: Value = serde_json::from_str(&out.join("\n")).unwrap();
    assert_eq!(json["groups"][0]["edited"], 2);
    assert_eq!(json["groups"][0]["median_files"], 2);

    let out = rig.ask(&["koji", "--item", "926"]).unwrap();
    assert!(
        out.last().unwrap().ends_with(".jsonl, 7 requests"),
        "{out:#?}"
    );
}

// pi's session file recorded for #922: two requests, the second of 20 input tokens.
#[test]
fn a_recorded_pi_session_reads_as_its_two_requests() {
    let rig = Rig::new();
    let session = "8b2d6e41-0c3f-4a97-b5e8-1d7f9a2c4e60";
    rig.call(Some(7), "reviewer", "pi", session, 0);
    let folder = rig.home.path().join(".pi/sessions/-Users-me-shep-kelpie");
    std::fs::create_dir_all(&folder).unwrap();
    let recorded = include_str!("../../../fixtures/pi-session-mcp.jsonl");
    std::fs::write(
        folder.join(format!("2026-10-15T19-59-36-998Z_{session}.jsonl")),
        recorded,
    )
    .unwrap();
    rig.started("pi", session, &folder);

    let out = rig.ask(&["koji", "--context"]).unwrap();
    let row: Vec<&str> = out[2].split_whitespace().collect();
    assert_eq!(
        row,
        ["reviewer", "pi", "1", "2", "10", "15", "20", "0%", "0%"]
    );
}
