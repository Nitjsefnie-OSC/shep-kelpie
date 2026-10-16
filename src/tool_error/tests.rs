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
