// Reading each tool call that failed out of a call's transcript, by what made it fail

use std::collections::BTreeMap;

use super::*;

// One real failed result for each kind each harness had, with the call that made it
const CLAUDE: &str = include_str!("../../fixtures/claude-transcript-tool-errors.jsonl");
const PI: &str = include_str!("../../fixtures/pi-session-tool-errors.jsonl");

fn counted(errors: &[ToolError]) -> BTreeMap<&'static str, u64> {
    let mut counts = BTreeMap::new();
    for error in errors {
        *counts.entry(error.kind.as_str()).or_insert(0) += 1;
    }
    counts
}

fn claude_of(text: &str) -> BTreeMap<&'static str, u64> {
    counted(&claude(text, 0))
}

fn pi_of(text: &str) -> BTreeMap<&'static str, u64> {
    counted(&pi(text, 0))
}

#[test]
fn a_refusal_by_kelpies_own_guard_or_confine_hook_is_a_guard_block() {
    let kinds = claude_of(CLAUDE);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(1),
        "the `confine` hook"
    );
    assert_eq!(
        kinds.get("hook-block").copied(),
        Some(1),
        "the project's own hook"
    );
}

#[test]
fn a_tool_the_call_was_never_given_counts_as_tool_not_given() {
    assert_eq!(claude_of(CLAUDE).get("tool-not-given").copied(), Some(1));
}

#[test]
fn a_skill_the_call_does_not_have_counts_as_unknown_skill() {
    assert_eq!(claude_of(CLAUDE).get("unknown-skill").copied(), Some(1));
}

#[test]
fn a_command_outside_the_calls_allow_list_counts_as_permission_refused() {
    assert_eq!(
        claude_of(CLAUDE).get("permission-refused").copied(),
        Some(1)
    );
}

#[test]
fn a_path_the_sandbox_refused_counts_as_path_refused() {
    assert_eq!(claude_of(CLAUDE).get("path-refused").copied(), Some(1));
}

#[test]
fn a_path_under_tmp_the_sandbox_refused_counts_as_tmp_refused() {
    assert_eq!(claude_of(CLAUDE).get("tmp-refused").copied(), Some(1));
}

#[test]
fn a_git_ref_git_cannot_lock_counts_as_git_ref_lock() {
    assert_eq!(claude_of(CLAUDE).get("git-ref-lock").copied(), Some(1));
}

#[test]
fn a_command_that_is_not_there_counts_as_command_not_found() {
    assert_eq!(claude_of(CLAUDE).get("command-not-found").copied(), Some(1));
}

#[test]
fn an_edit_whose_old_text_is_not_in_the_file_counts_as_edit_miss() {
    assert_eq!(claude_of(CLAUDE).get("edit-miss").copied(), Some(1));
}

#[test]
fn a_command_that_ran_past_its_limit_counts_as_timeout() {
    assert_eq!(claude_of(CLAUDE).get("timeout").copied(), Some(1));
}

#[test]
fn a_command_that_ran_and_failed_counts_as_other() {
    assert_eq!(claude_of(CLAUDE).get("other").copied(), Some(1));
}

#[test]
fn each_failed_result_in_the_recorded_call_counts_once() {
    assert_eq!(claude(CLAUDE, 0).len(), 12);
}

#[test]
fn a_refusal_by_a_pi_call_guard_extension_is_a_guard_block() {
    assert_eq!(pi_of(PI).get("guard-block").copied(), Some(1));
}

#[test]
fn a_path_a_pi_call_sandbox_refused_counts_as_path_refused() {
    assert_eq!(pi_of(PI).get("path-refused").copied(), Some(1));
}

#[test]
fn a_path_under_tmp_a_pi_call_sandbox_refused_counts_as_tmp_refused() {
    assert_eq!(pi_of(PI).get("tmp-refused").copied(), Some(1));
}

#[test]
fn a_git_ref_a_pi_call_git_cannot_lock_counts_as_git_ref_lock() {
    assert_eq!(pi_of(PI).get("git-ref-lock").copied(), Some(1));
}

#[test]
fn a_program_a_pi_tool_needs_that_is_not_there_counts_as_command_not_found() {
    assert_eq!(pi_of(PI).get("command-not-found").copied(), Some(1));
}

#[test]
fn an_edit_whose_old_text_a_pi_call_cannot_match_counts_as_edit_miss() {
    assert_eq!(pi_of(PI).get("edit-miss").copied(), Some(1));
}

#[test]
fn a_command_a_pi_call_ran_past_its_limit_counts_as_timeout() {
    assert_eq!(pi_of(PI).get("timeout").copied(), Some(1));
}

#[test]
fn a_command_a_pi_call_ran_and_failed_counts_as_other() {
    assert_eq!(pi_of(PI).get("other").copied(), Some(1));
}

#[test]
fn each_failed_result_in_the_recorded_pi_call_counts_once() {
    assert_eq!(pi(PI, 0).len(), 8);
}

// A recorded failure, given an MCP tool's name. No MCP failure was recorded in the
// four days the fixtures come from, so the name is the only part that is not real.
const MCP_TOOL: &str = "mcp__codebase_memory_mcp__search_graph";

#[test]
fn a_failed_mcp_tool_call_that_says_nothing_else_counts_as_mcp() {
    let text = CLAUDE.replace(
        "\"id\":\"toolu_016kwpRXt5T2RGDg6R2mrnkk\",\"name\":\"Bash\"",
        &format!("\"id\":\"toolu_016kwpRXt5T2RGDg6R2mrnkk\",\"name\":\"{MCP_TOOL}\""),
    );
    let kinds = claude_of(&text);
    assert_eq!(kinds.get("mcp").copied(), Some(1));
    assert_eq!(kinds.get("other"), None, "it is no longer `other`");
}

#[test]
fn a_failed_pi_mcp_tool_call_that_says_nothing_else_counts_as_mcp() {
    let text = PI.replace(
        "\"toolCallId\":\"call_bb621f4bc2904e6581188020\",\"toolName\":\"bash\"",
        &format!("\"toolCallId\":\"call_bb621f4bc2904e6581188020\",\"toolName\":\"{MCP_TOOL}\""),
    );
    let kinds = pi_of(&text);
    assert_eq!(kinds.get("mcp").copied(), Some(1));
    assert_eq!(kinds.get("other"), None, "it is no longer `other`");
}

#[test]
fn a_guard_refusal_on_a_result_that_also_holds_a_timeout_is_a_guard_block() {
    let text = PI.replace(
        r#""content":[{"type":"text","text":"Command timed out after 600 seconds"}]"#,
        &format!(
            r#""content":[{{"type":"text","text":"Command timed out after 600 seconds"}},{{"type":"text","text":"{GUARD_MARK}the build is red"}}]"#
        ),
    );
    let kinds = pi_of(&text);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(2),
        "the refusal of a call, and the feedback on a result"
    );
    assert_eq!(
        kinds.get("timeout"),
        None,
        "the first kind that matches wins"
    );
}

#[test]
fn a_pi_command_a_hook_refused_after_it_ran_is_a_guard_block() {
    let text = PI.replace(
        r#""content":[{"type":"text","text":"\n\n\nCommand exited with code 1"}]"#,
        &format!(
            r#""content":[{{"type":"text","text":"\n\n\nCommand exited with code 1"}},{{"type":"text","text":"{GUARD_MARK}the build is red"}}]"#
        ),
    );
    let kinds = pi_of(&text);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(2),
        "the refusal of a call, and the feedback on a result"
    );
    assert_eq!(
        kinds.get("other"),
        None,
        "the command ran, and the refusal is what counts"
    );
}

#[test]
fn a_command_that_prints_the_guard_mark_is_not_a_guard_refusal() {
    let text = PI.replace(
        r#""text":"\n\n\nCommand exited with code 1"#,
        &format!(r#""text":"{GUARD_MARK}the docs quote it\n\n\nCommand exited with code 1"#),
    );
    let kinds = pi_of(&text);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(1),
        "only the recorded refusal, which is a block of its own on a call that never ran"
    );
    assert_eq!(
        kinds.get("other").copied(),
        Some(1),
        "the command ran, so its own output is what failed"
    );
}

#[test]
fn output_that_quotes_the_guard_mark_is_not_a_guard_refusal() {
    let text = PI.replace(
        r#""text":"Command timed out after 600 seconds"#,
        r#""text":"the log line reads kelpie guard: no\nCommand timed out after 600 seconds"#,
    );
    let kinds = pi_of(&text);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(1),
        "only the recorded refusal"
    );
    assert_eq!(kinds.get("timeout").copied(), Some(1));
}

#[test]
fn a_result_that_succeeded_counts_nothing() {
    let clean = CLAUDE.replace("\"is_error\":true", "\"is_error\":false");
    assert_eq!(
        claude(&clean, 0),
        Vec::new(),
        "12 real results, none failed"
    );
    let clean = PI.replace("\"isError\":true", "\"isError\":false");
    assert_eq!(pi(&clean, 0), Vec::new(), "8 real results, none failed");
}

#[test]
fn a_hook_that_refused_an_mcp_tool_call_is_a_hook_block() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"PostToolUse:mcp__repo__ask hook error: [~/bin/after]: the post is not yours"}]}}
"#;
    assert_eq!(
        claude_of(line).get("hook-block").copied(),
        Some(1),
        "the tool's name is no kind of its own"
    );
}

#[test]
fn output_that_quotes_a_hook_refusal_is_not_one() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"Exit code 1\nthe docs say PreToolUse:Bash hook error: [cmd]: why"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("hook-block"),
        None,
        "not the result's own first line"
    );
    assert_eq!(kinds.get("other").copied(), Some(1));
}

#[test]
fn a_hook_named_guard_that_is_not_kelpies_is_a_hook_block() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"PreToolUse:Bash hook error: [/usr/local/bin/safety guard --strict]: the project's own gate"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("hook-block").copied(),
        Some(1),
        "the program is `safety`, not kelpie's"
    );
    assert_eq!(kinds.get("guard-block"), None);
}

#[test]
fn output_that_quotes_an_error_phrase_is_not_that_error() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"Exit code 1\ndocs/facts/tool-errors.md:12: each kind matches an error line (^error: cannot lock ref, a line naming /tmp/ and Operation not permitted)\nthe docs say Command timed out after 2m 0s and No such tool available"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("other").copied(),
        Some(1),
        "a command that ran and failed"
    );
    for quoted in [
        "git-ref-lock",
        "tmp-refused",
        "path-refused",
        "timeout",
        "tool-not-given",
        "unknown-skill",
    ] {
        assert_eq!(
            kinds.get(quoted),
            None,
            "{quoted} is not at the head of a line"
        );
    }
}

#[test]
fn a_pi_result_that_quotes_an_error_phrase_counts_as_other() {
    let line = r#"{"type":"message","id":"m1","parentId":"p1","timestamp":"2026-10-16T00:00:00.000Z","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"docs/facts/tool-errors.md:20: ripgrep (rg) is not available and could not be downloaded\n\n\nCommand exited with code 1"}],"isError":true,"durationMs":5,"timestamp":1792078844059}}
"#;
    let kinds = pi_of(line);
    assert_eq!(kinds.get("other").copied(), Some(1));
    assert_eq!(
        kinds.get("command-not-found"),
        None,
        "the phrase is at the end of a line that names the file it quoted"
    );
}

#[test]
fn a_kelpie_hook_under_a_path_with_a_space_is_still_a_guard_block() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"PreToolUse:Bash hook error: ['/Users/me/My Cargo/bin/shep-kelpie' 'guard' '--git-dir' '/x']: the fetch is not yours"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(1),
        "the quoted path is one word, not three"
    );
    assert_eq!(kinds.get("hook-block"), None);
}

#[test]
fn a_command_that_prints_matching_lines_from_files_is_not_those_errors() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"Exit code 1\nnotes.txt:3:Operation not permitted\ndocs/x.md:7:Command timed out after 2m\nsrc/webhook.rs:12: error: cannot lock ref\nnotes.txt:12: error: cannot lock ref\nnotes:3:No such file or directory\nthe grep ran and the build is red"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("other").copied(),
        Some(1),
        "a command that ran and failed"
    );
    for quoted in [
        "path-refused",
        "tmp-refused",
        "timeout",
        "git-ref-lock",
        "command-not-found",
    ] {
        assert_eq!(
            kinds.get(quoted),
            None,
            "{quoted} is what the grep matched, not what failed"
        );
    }
}

#[test]
fn a_program_error_that_names_a_file_still_counts_as_its_own_kind() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"Exit code 1\nmkdir: cbm-docs: Operation not permitted"}]}}
"#;
    assert_eq!(
        claude_of(line).get("path-refused").copied(),
        Some(1),
        "the program writes a space after each marker, so the message is read after them"
    );
}

#[test]
fn a_pi_command_that_prints_matching_lines_from_files_is_not_those_errors() {
    let line = r#"{"type":"message","id":"m1","parentId":"p1","timestamp":"2026-10-16T00:00:00.000Z","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"notes.txt:3:Operation not permitted\n\n\nCommand exited with code 1"}],"isError":true,"durationMs":5,"timestamp":1792078844059}}
"#;
    let kinds = pi_of(line);
    assert_eq!(kinds.get("other").copied(), Some(1));
    assert_eq!(kinds.get("path-refused"), None);
    assert_eq!(kinds.get("tmp-refused"), None);
}

#[test]
fn a_program_error_named_by_its_full_path_still_counts_as_its_own_kind() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"Exit code 2\n/usr/bin/env: 'python3': No such file or directory"}]}}
"#;
    assert_eq!(
        claude_of(line).get("command-not-found").copied(),
        Some(1),
        "a program's own name is no file and line number"
    );
}

#[test]
fn a_hook_command_that_holds_a_bracket_is_told_whole() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"PreToolUse:Bash hook error: [bash -c '[ -f .gate ] && exit 2']: the project's own gate"}]}}
"#;
    let errors = claude(line, 0);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, Kind::HookBlock);
    assert_eq!(
        errors[0].hook.as_deref(),
        Some("bash -c '[ -f .gate ] && exit 2'"),
        "the command is cut at the `]: ` before the reason, not at the `]` inside it"
    );
}

#[test]
fn kelpies_own_guard_under_a_path_with_a_bracket_is_still_a_guard_block() {
    let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","is_error":true,"tool_use_id":"t1","content":"PreToolUse:Bash hook error: ['/Users/me/a]b/shep-kelpie' 'guard' '--git-dir' '/x']: the fetch is not yours"}]}}
"#;
    let kinds = claude_of(line);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(1),
        "the whole command is read, bracket included"
    );
    assert_eq!(kinds.get("hook-block"), None);
}

#[test]
fn a_hook_refusal_names_the_hook_that_refused() {
    let errors = claude(CLAUDE, 0);
    let hooks: Vec<&str> = errors
        .iter()
        .filter_map(|error| error.hook.as_deref())
        .collect();
    assert_eq!(
        hooks,
        ["/opt/homebrew/bin/node ~/.claude/hooks/kelpie-gate.js"],
        "kelpie's own `confine` refusal is a guard block, which names no hook"
    );
}
