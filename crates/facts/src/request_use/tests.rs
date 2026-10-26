use super::After::{Gap, Growth, Start};
use super::*;

// A worker's session over three calls, each an hour or more after the last, from Claude Code
// 2.1.300: one request a call.
const SPACED: &str = include_str!("../../../../fixtures/claude-transcript-mcp-failed.jsonl");
// A session over two calls seconds apart, from Claude Code 2.1.301: three requests.
const CLOSE: &str = include_str!("../../../../fixtures/claude-transcript-mcp-tool-calls.jsonl");
// Six requests of one call from Claude Code 2.1.301, a line for each block of a reply: twelve
// lines, content left out.
const BLOCKS: &str = include_str!("../../../../fixtures/claude-transcript-request-sizes.jsonl");
// A pi 1.1.0 session of two requests, a second apart.
const PI: &str = include_str!("../../../../fixtures/pi-session-mcp.jsonl");

// Where the line that starts with `start`, `nth` from the first, begins
fn start_of(text: &str, start: &str, nth: usize) -> usize {
    text.match_indices(start)
        .filter(|(at, _)| *at == 0 || text.as_bytes()[at - 1] == b'\n')
        .nth(nth)
        .map(|(at, _)| at)
        .expect("the line")
}

fn request(context: u64, cache_write: u64, after: After) -> Request {
    Request {
        context,
        cache_write,
        after,
    }
}

#[test]
fn a_claude_session_s_first_request_is_its_start_and_one_hours_later_is_a_gap() {
    assert_eq!(
        claude(SPACED, 0),
        [
            request(18_576, 13_152, Start),
            request(69_169, 63_745, Gap),
            request(76_220, 70_796, Gap),
        ]
    );
}

#[test]
fn a_resumed_claude_call_counts_its_own_requests_after_the_gap_since_the_session_last_spoke() {
    let from = start_of(SPACED, "{\"type\":\"queue-operation\"", 1);
    assert_eq!(
        claude(SPACED, from),
        [request(69_169, 63_745, Gap), request(76_220, 70_796, Gap)]
    );
    let last = start_of(SPACED, "{\"type\":\"queue-operation\"", 2);
    assert_eq!(claude(SPACED, last), [request(76_220, 70_796, Gap)]);
}

#[test]
fn claude_requests_within_five_minutes_are_growth_across_a_call_s_end() {
    assert_eq!(
        claude(CLOSE, 0),
        [
            request(29_516, 18_449, Start),
            request(29_991, 475, Growth),
            request(30_271, 146, Growth),
        ]
    );
    let second = start_of(CLOSE, "{\"type\":\"queue-operation\"", 1);
    assert_eq!(claude(CLOSE, second), [request(30_271, 146, Growth)]);
}

#[test]
fn the_lines_of_one_claude_request_count_it_once() {
    let contexts: Vec<_> = claude(BLOCKS, 0).iter().map(|r| r.context).collect();
    assert_eq!(
        contexts,
        [17_692, 22_067, 38_142, 43_168, 45_113, 45_897],
        "twelve lines, six requests"
    );
    let writes: Vec<_> = claude(BLOCKS, 0).iter().map(|r| r.cache_write).collect();
    assert_eq!(writes, [12_268, 4_375, 16_075, 5_026, 1_945, 784]);
}

#[test]
fn a_claude_reply_that_sent_nothing_is_no_request() {
    let synthetic = r#"{"type":"assistant","timestamp":"2026-10-15T21:09:00.000Z","message":{"id":"msg_x","model":"<synthetic>","role":"assistant","content":[{"type":"text","text":""}],"usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}"#;
    let text = format!("{BLOCKS}{synthetic}\n");
    assert_eq!(claude(&text, 0).len(), 6);
}

#[test]
fn claude_requests_without_a_readable_time_are_growth() {
    let text = BLOCKS.replace("2026-10-15T21:03:39", "yesterday");
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(after, [Start, Growth, Growth, Growth, Growth, Growth]);
}

#[test]
fn a_file_shorter_than_where_the_call_began_is_a_new_one_and_all_its_own() {
    assert_eq!(claude(CLOSE, CLOSE.len() + 100).len(), 3);
}

#[test]
fn pi_reads_a_request_from_each_assistant_message() {
    assert_eq!(
        pi(PI, 0),
        [request(10, 0, Start), request(20, 0, Growth)],
        "input plus cache read and write"
    );
    let second = start_of(PI, "{\"type\":\"message\",\"id\":\"f618c3f6\"", 0);
    assert_eq!(pi(PI, second), [request(20, 0, Growth)]);
}

#[test]
fn a_pi_request_over_five_minutes_after_the_one_before_is_a_gap() {
    let later = PI
        .replace("2026-10-15T19:59:41.430Z", "2026-10-15T20:05:41.431Z")
        .replace(
            "\"input\":20,\"output\":5,\"cacheRead\":0,\"cacheWrite\":0",
            "\"input\":20,\"output\":5,\"cacheRead\":3000,\"cacheWrite\":500",
        );
    assert_eq!(
        pi(&later, 0),
        [request(10, 0, Start), request(3_520, 500, Gap)]
    );
}

// A Claude Code reply of one block at `time` (a minute of 2026-10-15 at 22h), its usage reading
// `read` and writing `five` tokens to the five-minute cache and `hour` to the one-hour cache
fn claude_line(id: &str, minute: u32, read: u64, five: u64, hour: u64) -> String {
    let write = five + hour;
    format!(
        r#"{{"type":"assistant","timestamp":"2026-10-15T22:{minute:02}:00.000Z","message":{{"id":"{id}","role":"assistant","usage":{{"input_tokens":3,"cache_read_input_tokens":{read},"cache_creation_input_tokens":{write},"cache_creation":{{"ephemeral_5m_input_tokens":{five},"ephemeral_1h_input_tokens":{hour}}}}}}}}}
"#
    )
}

#[test]
fn a_claude_request_after_over_five_minutes_is_growth_while_the_one_hour_cache_held_the_write() {
    // Six and then fifty-nine minutes after a write to the cache that keeps it for an hour.
    let text = [
        claude_line("a", 0, 0, 0, 1_000),
        claude_line("b", 6, 1_000, 0, 500),
        claude_line("c", 59, 1_500, 0, 200),
    ]
    .concat();
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(after, [Start, Growth, Growth]);
}

#[test]
fn a_claude_request_after_over_an_hour_is_a_gap_whatever_the_cache() {
    let text = [
        claude_line("a", 0, 0, 0, 1_000),
        claude_line("b", 30, 1_000, 0, 500),
    ]
    .concat()
        + &claude_line("c", 0, 1_500, 0, 200).replace("22:00:00", "23:31:00");
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(after, [Start, Growth, Gap]);
}

#[test]
fn a_claude_request_after_over_five_minutes_is_a_gap_once_the_cache_kept_a_write_five_minutes() {
    // A write to both caches lasts as long as the shorter.
    let text = [
        claude_line("a", 0, 0, 300, 700),
        claude_line("b", 6, 1_000, 0, 0),
    ]
    .concat();
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(after, [Start, Gap]);
}

#[test]
fn a_claude_request_that_wrote_nothing_leaves_the_last_cache_time_standing() {
    let text = [
        claude_line("a", 0, 0, 0, 1_000),
        claude_line("b", 2, 1_000, 0, 0),
        claude_line("c", 10, 1_000, 0, 0),
    ]
    .concat();
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(
        after,
        [Start, Growth, Growth],
        "the hour still stands at ten minutes"
    );
}

#[test]
fn a_claude_request_with_no_split_is_a_gap_after_five_minutes() {
    let text = [
        claude_line("a", 0, 0, 0, 1_000),
        claude_line("b", 2, 1_000, 0, 0),
    ]
    .concat()
    .replace(
        r#","cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1000}"#,
        "",
    );
    let text = text + &claude_line("c", 9, 1_000, 0, 0);
    let after: Vec<_> = claude(&text, 0).iter().map(|r| r.after).collect();
    assert_eq!(after, [Start, Growth, Gap]);
}

// A Codex 0.159.3 call that compacted: five requests, the fourth the compaction's own.
const CODEX: &str = include_str!("../../../../fixtures/codex-rollout-compacted.jsonl");

#[test]
fn codex_reads_a_request_from_each_token_usage_record_the_compaction_s_included() {
    assert_eq!(
        codex(CODEX, 0),
        [
            request(19_244, 0, Start),
            request(19_347, 0, Growth),
            request(29_249, 0, Growth),
            request(29_387, 0, Growth),
            request(19_478, 0, Growth),
        ],
        "the fourth is the compaction's, which has no `token_count` event of its own"
    );
}

#[test]
fn a_resumed_codex_call_counts_its_own_requests() {
    let second = start_of(CODEX, "{\"timestamp\":\"2026-10-21T15:40:55", 0);
    assert_eq!(codex(CODEX, second).len(), 4);
}

#[test]
fn a_codex_request_over_five_minutes_after_one_that_wrote_the_cache_is_a_gap() {
    let later = CODEX
        .replace("2026-10-21T15:40:55.216Z", "2026-10-21T15:46:55.216Z")
        .replace(
            "\"input_tokens\":19244,\"cached_input_tokens\":0,\"cache_write_input_tokens\":0",
            "\"input_tokens\":19244,\"cached_input_tokens\":0,\"cache_write_input_tokens\":900",
        );
    let found = codex(&later, 0);
    assert_eq!(found[0], request(19_244, 900, Start));
    assert_eq!(found[1].after, Gap);
}
