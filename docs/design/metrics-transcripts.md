# Metrics from transcripts

What a call's transcript says once it ends, as series, and the reports that read transcripts. The rest of kelpie's numbers are in `metrics.md`. ADR 0012, #920, #924.

## Series

MCP servers, read from a call's transcript once it ends: the agents a call runs on are wrapped (`src/runner/transcript_count.rs`), which find the session's file through `Agents::transcript` and parse it (`src/mcp_use.rs`). A call that wrote nothing to it counts nothing.

- `kelpie_mcp_servers_total{server,outcome}`: each server in the call's reach, `connected` or `failed`. Claude Code writes how its servers stand only when that changes, so the transcript's last such line is how they stood; `needsAuth` is `failed`, `pending` is not counted. A failed server is a standing warning naming the call and its error code, ended when a later call finds it connected. Claude Code only: pi records no failure (`docs/facts/mcp-servers.md`).
- `kelpie_mcp_tool_calls_total{server}`: the call's own `mcp__<server>__<tool>` calls, from where its file stood when it began; each server it was given is sent, at 0 if never called. Claude Code and pi.

Requests, read from the same file by the same wrapper (`src/request_use.rs`, `docs/facts/transcript-requests.md`), for the requests the call itself made, of the worker, the reviewer and the project manager, the long sessions (the one-shot roles' would hold nine series for each harness more than the cap of 200 has room for). Claude Code and pi (`harness.request-usage`):

- `kelpie_request_context_tokens{role,harness,le}`: requests by context size (input, cache read and cache write), as cumulative counters with `le` 25000, 50000, 100000, 150000, 200000 and `+Inf`, which `histogram_quantile` reads over a `rate`. The channel has no histogram type, so this counter has no `_total`. All six are sent with a role and harness's first request.
- `kelpie_cache_write_tokens_total{role,harness,after}`: tokens written to the cache, `after` `start` (the session's first request), `gap` (over 5 minutes since the request before, which may be an earlier call's) or `growth`. All three are sent with the first. A role and harness holds 9 series between the two.
- A request is one however many lines Claude Code wrote it as; a reply that sent nothing is none.
- A worker's turn carried through a handover is counted by the image that adopts it. `CarriedTurn` holds a `Mark` (the transcript's folder, the session and its length as the turn began), and the image counts the requests past it when the turn ends. Its MCP servers and tool calls are not counted, since the mark holds no servers.

## Reports from transcripts

- `shep kelpie usage <project> --item|--context|--reads` (`--json` too) join the ledger to transcripts by its `session` field, with no shepherd. A transcript is found through the `session-started` steps in `steps.jsonl`, never a guessed folder. Requests are read for Claude Code (one per `message.id`, subagent lines left out) and pi; Codex is found, not read (#757). `src/usage/session.rs`, `src/usage/requests.rs`, #926
