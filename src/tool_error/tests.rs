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
    assert_eq!(kinds.get("guard-block").copied(), Some(1), "the `confine` hook");
    assert_eq!(kinds.get("hook-block").copied(), Some(1), "the project's own hook");
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
    assert_eq!(claude_of(CLAUDE).get("permission-refused").copied(), Some(1));
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
fn a_guard_refusal_that_also_holds_a_timeout_is_a_guard_block() {
    let text = PI.replace(
        "\"text\":\"Command timed out after 600 seconds\"",
        &format!("\"text\":\"{GUARD_MARK}Command timed out after 600 seconds\""),
    );
    let kinds = pi_of(&text);
    assert_eq!(
        kinds.get("guard-block").copied(),
        Some(2),
        "the refusal, and the guard's own"
    );
    assert_eq!(kinds.get("timeout"), None, "the first kind that matches wins");
}

#[test]
fn a_result_that_succeeded_counts_nothing() {
    let clean = CLAUDE.replace("\"is_error\":true", "\"is_error\":false");
    assert_eq!(claude(&clean, 0), Vec::new(), "12 real results, none failed");
    let clean = PI.replace("\"isError\":true", "\"isError\":false");
    assert_eq!(pi(&clean, 0), Vec::new(), "8 real results, none failed");
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
