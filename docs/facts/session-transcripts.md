# Session transcripts

- Claude Code writes one line per content block (read from a worker's own transcript on 2026-10-10, Claude Code 2.1.x).
  - One assistant message with a thought, a sentence and a tool call is three lines, each with its own `timestamp`.
  - A prompt is a `user` line whose `message.content` is a string. A tool's result is a later `user` line whose content is a `tool_result` block, its `content` a string or a list of text blocks and `is_error` set on a failure.
  - A thought's `thinking` is often empty, with only a `signature`, so `watch` shows no line for it. `queue-operation` lines repeat the prompt and `attachment` lines carry reminders, and `watch` skips both.
- Codex's rollout and pi's session file were written from what each program documents; no transcript of either was at hand when `watch` was built.
  - Codex: `response_item` lines, whose payload is a `message`, `reasoning`, `function_call` with its arguments as a JSON string, `custom_tool_call` or an output, and `event_msg` lines that repeat them.
  - pi: `message` entries whose role is `user`, `assistant` (text, thinking and `toolCall` blocks) or `toolResult`.
  - A line either parser cannot read is passed over, so a format that differs costs lines, not the stream. A recorded rollout and session file belong in `fixtures/` the first time a codex or pi worker runs.
