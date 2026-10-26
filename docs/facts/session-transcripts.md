# Session transcripts

- Claude Code writes one line per content block (read from a worker's own transcript on 2026-10-10, Claude Code 2.1.x).
  - One assistant message with a thought, a sentence and a tool call is three lines, each with its own `timestamp`.
  - A prompt is a `user` line whose `message.content` is a string. A tool's result is a later `user` line whose content is a `tool_result` block, its `content` a string or a list of text blocks and `is_error` set on a failure.
  - A thought's `thinking` is often empty, with only a `signature`, so `watch` shows no line for it. `queue-operation` lines repeat the prompt and `attachment` lines carry reminders, and `watch` skips both.
- Codex's rollout and pi's session file were written from what each program documents; no transcript of either was at hand when `watch` was built.
  - Codex: `response_item` lines, whose payload is a `message`, `reasoning`, `function_call` with its arguments as a JSON string, `custom_tool_call` or an output, and `event_msg` lines that repeat them.
  - pi: `message` entries whose role is `user`, `assistant` (text, thinking and `toolCall` blocks) or `toolResult`.
  - A line either parser cannot read is passed over, so a format that differs costs lines, not the stream. A recorded rollout and session file belong in `fixtures/` the first time a codex or pi worker runs.
- A Claude Code request is several lines, which `usage --context` counts once (read from a worker's own transcript on 2026-10-15, Claude Code 2.1.x, for #926).
  - Every `assistant` line a request wrote carries the request's `message.id` and the same `message.usage`: in a session of 43 requests, none had lines whose usage differed. A request's context is `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`.
  - A subagent's lines carry `isSidechain: true` and are not the session's requests.
  - `fixtures/claude-transcript-requests.jsonl` is a trimmed excerpt of it: seven requests, two reads and the edits after them, long inputs and results cut, the home folder written as `/Users/me`.
- A Codex rollout holds a `token_usage_record` line for each request (read from `fixtures/codex-rollout-compacted.jsonl`, Codex 0.159.3, for #1138).
  - Its `payload.usage` is that request's tokens and its `payload.response_id` is the request's, so a request is counted once by `response_id`. The `event_msg` `token_count` lines repeat it but leave out a compaction's own request, which a `token_usage_record` holds (#1128). `input_tokens` holds `cached_input_tokens` already.
  - A Codex rollout names no file a tool call read. The recording is trimmed to its token lines, so kelpie takes this from Codex's tools, which read a file through a shell command and edit it with a patch; `usage --reads` shows no touches for it, and a session counts as having edited none.
- pi writes one `message` entry per request, with `message.usage` as `input`, `output`, `cacheRead` and `cacheWrite`, and its tool calls as `toolCall` blocks named `read`, `edit` and `write` whose `arguments.path` may start with `@` (`fixtures/pi-session-mcp.jsonl`, `crates/adapters/src/adapters/pi/guard_tests.rs`).
