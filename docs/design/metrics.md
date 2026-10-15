# Metrics

Kelpie's numbers, live, on shep's metrics endpoint. ADR 0012, #920.

## Built

- Runner and dog send channel samples (`shep_channel::Shepherd::metric_with`); shep's metrics dog serves them on `/metrics` (default `127.0.0.1:9615`) with the sender's labels first: `sheep` (the project), `id`, `fold` for a runner; `dog`, `source` for the dog. `src/metrics.rs`
- Through the `Sink` port (`src/ports/metrics.rs`): the real one is the runner's `Shepherd`, a test holds `RecordedSink` and asserts the samples sent. Sending never blocks or fails; a full outbox drops a sample, and the next carries the total.
- Counters are kept whole in kelpie's memory (`Recorder`) and each change sends the new total. A runner start begins them at zero, as Prometheus expects of a restart.
- A runner sends at most `SERIES_CAP` (200) series, under the dog's 256 a sender, whose headroom other series are using; a new series past it is not sent and counts in `kelpie_series_dropped_total`, with one log line per name.
- Names `kelpie_*`, snake case; counters end `_total`. Labels only from bounded sets: `role`, `kind`, `harness`, `model`, `ended`, `phase`, `feature`, `outcome`, `type`, `le`, `after`, `server` (kelpie's own servers by name, any other `repo`). Never an issue, pull request, session or path.

## Series

Calls, counted as each ledger line is appended (`Ledger::append`, `src/usage.rs`), the file written or not. Lines a stopping runner writes, imports and `shep kelpie issue`'s are not counted:

- `kelpie_calls_total{role,kind,harness,ended}`
- `kelpie_tokens_total{role,harness,model,type}`: `type` is `input`, `output`, `cache_read`, `cache_write`
- `kelpie_cost_usd_total{role,harness,model}`, priced calls only; `kelpie_unpriced_calls_total{role,harness}`
- `kelpie_call_seconds_total{role,kind}`, beside `kelpie_calls_total` for a mean

Work items:

- `kelpie_work_items{phase}` gauge, sent after each step from the state file
- `kelpie_work_items_finished_total{outcome}`: `merged`, `closed`, `dropped`, from the ledger's `finished` line
- `kelpie_review_rounds_total`, `kelpie_fix_turns_total`, `kelpie_rulings_total`, from the same line
- `kelpie_work_item_seconds_total{outcome}`, the wall time of finished items

Features: whether what a project turned on runs.

- `kelpie_feature_enabled{feature}` gauge (1 or 0) at open and each reread: `code_graph`, `browser`, `dev_server` (browser on and `main`'s launch file selects a configuration), `hooks`
- `kelpie_feature_runs_total{feature,outcome}`: `code_graph` index `ok`, `failed`, `skipped` (no binary), `ended` (call ended first). `src/runner/code_graph.rs`
- `browser`: the bridge opened for a turn `ok`, `failed`, `skipped` (tools not installed). `src/runner/browser.rs`
- `dev_server`: `ok` (answered within `READY_WITHIN`), `failed` (could not be planned or started, or silent), `skipped` (no launch file on `main`, or none configured). `src/runner/dev_server.rs`
- `hooks`: each hook the runner runs, `ok` (exit 0 or a deliberate exit 2) or `failed`; no label per hook or point. The hooks carry the recorder (`Hooks::counted_by`); a `kelpie guard` or `kelpie confine` process has none, so the worker's own `commit`, `push` and `command` hooks are not counted. `src/hooks.rs`
- `KelpieFeatureNeverWorks` only counts a feature while `kelpie_feature_enabled` is 1.

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

## Alerts and dashboard

- `assets/prometheus/kelpie-rules.yml`: a feature failed or skipped since the runner started with no `ok` run (30m); a feature failing in over half its runs in 6h; an MCP server failing to connect in over half its calls in 6h; a role's calls `failed`, `timed-out` or `unreadable` in over a quarter of 6h's calls; a runner's series dropped.
- `assets/grafana/kelpie.json`: spend by role and model, tokens by type, calls by how they ended, work items by phase, features, MCP servers and tool calls, request context sizes and cache writes by what came before, mean call time.
- `docs/guide/metrics.md`: scrape config, the series, what each alert means.

## Not built

- MCP servers and tool calls for Codex (#757) and goose (#784).
- Feature runs of the hooks `kelpie guard` and `kelpie confine` run inside a worker's sandbox: another process, with no recorder.
- Requests for Codex (#757) and goose (#784).
- The dog's series: leases held, waits, summons, forge holds. #925
- Canary: a real issue a night through each harness with every feature on, checked from its transcript. #927
