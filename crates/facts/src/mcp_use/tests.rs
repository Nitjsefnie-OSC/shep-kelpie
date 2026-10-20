use super::*;

// A worker's session over four calls, from Claude Code 2.1.300: the code graph failed to connect
// in its first, the next two wrote no state of their servers, and the last one's state lists no
// failure.
const FAILED: &str = include_str!("../../../../fixtures/claude-transcript-mcp-failed.jsonl");
// A session given the code graph, which connected, and a server whose command is not there, over
// two calls, from Claude Code 2.1.301. The first called the graph's `list_projects`.
const TOOL_CALLS: &str =
    include_str!("../../../../fixtures/claude-transcript-mcp-tool-calls.jsonl");
// A reviewer's call from Claude Code 2.1.301 whose only server, the code graph, connected and whose
// tools were not deferred: it wrote no state of its servers, and its second prompt snapshot lists
// the graph's tools.
const CONNECTED: &str = include_str!("../../../../fixtures/claude-transcript-mcp-connected.jsonl");
// A pi 1.1.0 session given the code graph, which connected, and the server whose command is not
// there, whose model called the graph's `list_projects`. Its lines hold no system message of its
// tools.
const PI: &str = include_str!("../../../../fixtures/pi-session-mcp.jsonl");
// A pi 1.1.0 worker session given the code graph, recorded after #949, whose system message lists
// the graph's 17 tools in `toolsAdded`. Cut to its system message, each tool kept to its name and
// the start of its description.
const PI_CONNECTED: &str = include_str!("../../../../fixtures/pi-session-mcp-connected.jsonl");

// Where the line that starts with `start`, `nth` from the first, begins
fn start_of(text: &str, start: &str, nth: usize) -> usize {
    text.match_indices(start)
        .filter(|(at, _)| *at == 0 || text.as_bytes()[at - 1] == b'\n')
        .nth(nth)
        .map(|(at, _)| at)
        .expect("the line")
}

fn failed(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    (pairs.iter())
        .map(|(name, why)| ((*name).to_owned(), (*why).to_owned()))
        .collect()
}

#[test]
fn a_claude_call_whose_server_failed_to_connect_reads_it_failed_with_its_error_code() {
    let end = start_of(FAILED, "{\"type\":\"queue-operation\"", 1);
    let used = claude(&FAILED[..end], 0);
    let stood = used
        .servers
        .expect("the transcript says how its servers stood");
    let expected = [("codebase-memory-mcp", "CONNECTION_CLOSED")];
    assert_eq!(stood.failed, failed(&expected));
    assert!(stood.pending.is_empty());
    assert!(used.tool_calls.is_empty(), "{:?}", used.tool_calls);
}

#[test]
fn a_resumed_claude_call_that_writes_no_new_state_keeps_the_failure_it_had() {
    let end = start_of(FAILED, "{\"type\":\"queue-operation\"", 2);
    let text = &FAILED[..end];
    let from = start_of(text, "{\"type\":\"queue-operation\"", 1);
    let stood = claude(text, from).servers.expect("an earlier call's state");
    let expected = [("codebase-memory-mcp", "CONNECTION_CLOSED")];
    assert_eq!(stood.failed, failed(&expected));
}

#[test]
fn a_later_claude_state_that_lists_no_failure_ends_it() {
    let from = start_of(FAILED, "{\"type\":\"queue-operation\"", 2);
    let used = claude(FAILED, from);
    assert_eq!(
        used.servers.expect("the last call's state").failed,
        failed(&[])
    );
}

#[test]
fn a_claude_call_s_tool_calls_are_counted_by_server_and_its_own_tools_are_not() {
    let from = start_of(TOOL_CALLS, "{\"type\":\"queue-operation\"", 1);
    let first_call = claude(&TOOL_CALLS[..from], 0);
    let expected: BTreeMap<String, u64> = [("codebase-memory-mcp".to_owned(), 1)].into();
    assert_eq!(
        first_call.tool_calls, expected,
        "ToolSearch is Claude Code's own"
    );
    let stood = first_call.servers.expect("the state");
    let broken = stood.failed.get("broken").map(String::as_str);
    assert_eq!(broken, Some("ENOENT"));
    assert!(!stood.failed.contains_key("codebase-memory-mcp"));

    let resumed = claude(TOOL_CALLS, from);
    assert!(
        resumed.tool_calls.is_empty(),
        "the first call's are not the second's"
    );
    let stood = resumed.servers.expect("the first call's state still holds");
    assert!(stood.failed.contains_key("broken"));
}

#[test]
fn a_claude_transcript_with_no_state_of_its_servers_says_nothing_of_them() {
    let used = claude(
        "{\"type\":\"queue-operation\",\"operation\":\"enqueue\"}\n",
        0,
    );
    assert_eq!(used.servers, None);
}

#[test]
fn a_claude_call_whose_servers_all_connected_and_wrote_no_state_reads_none_failed() {
    let stood = claude(CONNECTED, 0)
        .servers
        .expect("its prompt snapshot lists a server's tools");
    assert_eq!(stood, Stood::default());
}

#[test]
fn a_prompt_snapshot_with_no_server_s_tools_says_nothing_of_the_servers() {
    let line = r#"{"type":"attachment","attachment":{"type":"prompt_snapshot","tools":[{"name":"Read"}]}}"#;
    assert_eq!(claude(line, 0).servers, None);
}

#[test]
fn needs_auth_is_a_failure_and_pending_is_neither() {
    let line = r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta","pendingMcpServers":["slow"],"needsAuthMcpServers":["sentry"],"failedMcpServers":[]}}"#;
    let stood = claude(line, 0).servers.expect("the state");
    assert_eq!(stood.failed, failed(&[("sentry", NEEDS_AUTH)]));
    assert_eq!(stood.pending, ["slow".to_owned()]);
}

#[test]
fn a_pi_session_s_tool_calls_are_counted_by_server_and_it_says_nothing_of_its_servers() {
    let used = pi(PI, 0);
    let expected: BTreeMap<String, u64> = [("codebase_memory_mcp".to_owned(), 1)].into();
    assert_eq!(used.tool_calls, expected);
    assert_eq!(
        used.servers, None,
        "pi records no server that failed to connect"
    );
    assert_eq!(
        used.reached
            .expect("pi says which of its servers it reached")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["codebase_memory_mcp"],
        "it lists no system message of its tools, and answered the graph's call"
    );
    let from = start_of(PI, "{\"type\":\"message\"", 3);
    assert!(
        pi(PI, from).tool_calls.is_empty(),
        "only the lines from `from` on"
    );
}

#[test]
fn a_pi_session_that_lists_a_server_s_tools_in_its_system_message_reads_it_loaded() {
    let used = pi(PI_CONNECTED, 0);
    let loaded = used.reached.expect("its system message lists its tools");
    assert_eq!(
        loaded.iter().map(String::as_str).collect::<Vec<_>>(),
        ["codebase_memory_mcp"],
        "the graph's 17 tools, and pi's own tools name no server"
    );
    assert!(
        used.servers.is_none(),
        "pi records no server that failed to connect"
    );
}

#[test]
fn a_resumed_pi_call_reads_the_tools_its_session_loaded_in_an_earlier_call() {
    let text = format!(
        "{PI_CONNECTED}{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[]}}}}\n"
    );
    let used = pi(&text, PI_CONNECTED.len());
    assert!(
        used.reached
            .expect("the session's own system message")
            .contains("codebase_memory_mcp"),
        "pi writes `toolsAdded` once, in the session's first system message"
    );
    assert!(
        used.tool_calls.is_empty(),
        "and only the call's own tool calls"
    );
}

// pi's `system` messages for one server's tools: added, then one of them gone, then the last one,
// each removal written by re-registering that tool hidden
const ADDED: &str = r#"{"type":"message","message":{"role":"system","content":"","toolsAdded":[{"name":"mcp__docs__search"},{"name":"mcp__docs__fetch"},{"name":"read"}]}}"#;
const ONE_GONE: &str = r#"{"type":"message","message":{"role":"system","content":"","toolsRemoved":[{"name":"mcp__docs__fetch"}]}}"#;
const ALL_GONE: &str = r#"{"type":"message","message":{"role":"system","content":"","toolsRemoved":[{"name":"mcp__docs__search"}]}}"#;
const BOTH_GONE: &str = r#"{"type":"message","message":{"role":"system","content":"","toolsRemoved":[{"name":"mcp__docs__search"},{"name":"mcp__docs__fetch"}]}}"#;
// A call to one of the server's tools, and the answer to it. pi writes `isError` false for one the
// server answered, and true for one it did not or answered with an error.
const CALLED: &str = concat!(
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_1","name":"mcp__docs__search","arguments":{}}]}}"#,
    "\n",
    r#"{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","toolName":"mcp__docs__search","content":[{"type":"text","text":"3 hits"}],"isError":false}}"#,
    "\n",
);
const ANSWERED_WITH_AN_ERROR: &str = concat!(
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_1","name":"mcp__docs__search","arguments":{}}]}}"#,
    "\n",
    r#"{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","toolName":"mcp__docs__search","content":[{"type":"text","text":"no such tool"}],"isError":true}}"#,
    "\n",
);

#[test]
fn a_pi_session_that_removes_a_server_s_tools_reads_it_gone_and_one_tool_left_keeps_it() {
    let used = pi(&format!("{ADDED}\n{ONE_GONE}\n"), 0);
    let loaded = used.reached.expect("its system message adds tools");
    assert_eq!(
        loaded.iter().map(String::as_str).collect::<Vec<_>>(),
        ["docs"],
        "one of the server's tools is still there"
    );

    let used = pi(&format!("{ADDED}\n{ONE_GONE}\n{ALL_GONE}\n"), 0);
    assert!(
        used.reached
            .expect("its system message adds tools")
            .is_empty(),
        "none of them left, whatever an earlier line added"
    );
}

#[test]
fn a_pi_session_that_adds_a_server_s_tools_again_after_removing_them_reads_it_there() {
    let used = pi(&format!("{ADDED}\n{ONE_GONE}\n{ALL_GONE}\n{ADDED}\n"), 0);
    assert!(
        used.reached
            .expect("its system message adds tools")
            .contains("docs"),
        "the last word is the addition"
    );
}

#[test]
fn a_pi_session_that_answers_a_server_s_tool_call_reads_it_reached_without_a_system_message() {
    let used = pi(CALLED, 0);
    assert_eq!(
        used.reached
            .expect("it answered one of the server's tools")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["docs"],
        "a tool the server answered was in the session"
    );
}

#[test]
fn a_pi_session_whose_tool_call_its_server_answered_with_an_error_reads_nothing_of_it() {
    assert!(
        ANSWERED_WITH_AN_ERROR
            .lines()
            .any(|line| line.contains("\"isError\":true")),
        "the stand-in's line"
    );
    let used = pi(ANSWERED_WITH_AN_ERROR, 0);
    assert!(
        used.reached
            .expect("pi says which servers it reached")
            .is_empty(),
        "an error says the tool was called, not that the server stood"
    );
}

#[test]
fn a_pi_session_s_last_word_about_a_server_is_how_it_reads_it() {
    let used = pi(&format!("{ADDED}\n{BOTH_GONE}\n{CALLED}"), 0);
    assert!(
        used.reached
            .expect("the call it answered is the last word")
            .contains("docs"),
        "the server answered after the removal line"
    );

    let used = pi(&format!("{CALLED}{ADDED}\n{BOTH_GONE}"), 0);
    assert!(
        used.reached
            .expect("the removal is the last word")
            .is_empty(),
        "and the tools went after that"
    );
}

#[test]
fn a_tool_s_server_part_is_named_as_each_harness_names_it() {
    assert_eq!(
        claude_tool_part("codebase-memory-mcp"),
        "codebase-memory-mcp"
    );
    assert_eq!(claude_tool_part("claude.ai Gmail"), "claude_ai_Gmail");
}
