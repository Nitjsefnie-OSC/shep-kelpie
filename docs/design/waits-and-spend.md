# Waits and spend

A model that is not there yet, what a role spends, and how an agent's usage is read. Agent files, harnesses and gateways are in agents.md.

- Local implementer (`usage: none`) can be default; without a gateway keep `active_items` 1. #381
- Turn awaiting its model marked `waiting` in memory; with `agents.fallback_after`, waiting first turn moves to next free implementer; later or pinned turns never (lose session). A turn's last output is read from the call's stdout file for pi (`src/adapters/pi/activity.rs`: none until a model event, a `message_update`, `tool_execution_*`, `compaction_*`, `auto_retry_*` or an assistant `message_end` with content, so the opening lines pi writes at once do not count; a file past 1 MiB counts unread; a last line without its newline is not read) and Codex, not pi's session file, which moves once per message. #381, #768, `src/runner/flight/waiting.rs`
- Unreachable pi, goose or Codex host (or an overloaded Codex provider) retried 30s to 5 min for 30 min before parking. Codex's reading is `AgentError::Unreachable` from `parse_result`; the leading-status rule is shared in `src/adapters/unreachable.rs`. #457, #776, `src/runner/turn/unreachable.rs`, `src/adapters/codex.rs`
- `usage` (`claude`, `codex`, `none`) must match harness; `none` never paced; a gateway queues its calls, a `url` model is unqueued. #129, #502
- Spend = tokens per role, dollars only where reported; unpriced calls counted (zero never reads as free). #129
