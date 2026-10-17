# Metrics

Kelpie's numbers, live, on shep's metrics endpoint. ADR 0012, #920.

## Built

- Runner and dog send channel samples (`shep_channel::Shepherd::metric_with`); shep's metrics dog serves them on `/metrics` (default `127.0.0.1:9615`) with the sender's labels first: `sheep` (the project), `id`, `fold` for a runner; `dog`, `source` for the dog. `src/metrics.rs`
- Through the `Sink` port (`src/ports/metrics.rs`): the real one is the runner's `Shepherd`, a test holds `RecordedSink` and asserts the samples sent. Sending never blocks or fails; a full outbox drops a sample, and the next carries the total.
- Counters are kept whole in kelpie's memory (`Recorder`) and each change sends the new total. A runner start begins them at zero, as Prometheus expects of a restart.
- A runner sends at most `SERIES_CAP` (200) series, under the dog's 256 a sender; a new series past it is not sent and counts in `kelpie_series_dropped_total`, with one log line per name.
- Names `kelpie_*`, snake case; counters end `_total`. Labels only from bounded sets: `role`, `kind`, `agent` and `effort` (named in settings), `harness`, `model`, `ended`, `account`, `window`, `phase`, `feature`, `outcome`, `type`, `tests`, `le`, `after`, `server` (kelpie's own servers by name, any other `repo`). Never an issue, pull request, session or path.

## Series

Calls, counted as each ledger line is appended (`Ledger::append`, `src/usage.rs`), the file written or not. Lines a stopping runner writes, imports and `shep kelpie issue`'s are not counted:

- `kelpie_calls_total{role,kind,agent,harness,effort,ended}`: `agent` is the settings' name for it (`codex`, `coderabbit`, `qwen`), and `agent` and `effort` read `unknown` where the line has none (imported lines, a local round's effort)
- `kelpie_tokens_total{role,agent,harness,model,effort,type}`: `type` is `input`, `output`, `cache_read`, `cache_write`
- `kelpie_cost_usd_total{role,agent,harness,model,effort}`, priced calls only; `kelpie_unpriced_calls_total{role,harness}`
- `kelpie_compactions_total{role,harness}`: a compaction the harness did by itself partway through a call, from the ledger line's `compactions`. Claude Code: its compaction hook (`src/adapters/claude/settings.rs`) leaves a mark in the call's scratch folder, counted when the call ends; only a worker with a `compact_window` has the hook, and a call another image adopted counts none. pi: each `compaction_end` of a turn that made a summary. Kelpie's own `/compact` is `kind="compact"` in `kelpie_calls_total`, not here. Codex and goose: none yet
- `kelpie_call_seconds_total{role,kind}`, beside `kelpie_calls_total` for a mean

Plan limits: gauges the pacer sets whenever it reads an account's usage (`Runner::pace`, `src/runner/pace.rs`), not when the reading fails. `account` is `claude` or `codex` (`Account::as_str`). `src/metrics/plan.rs`

- `kelpie_plan_used_ratio{account,window}`: `window` is `session` (5 hours) or `week`, 0 to 1
- `kelpie_plan_resets_at_seconds{account,window}`: Unix seconds
- `kelpie_plan_allowance_ratio{account}`, `kelpie_plan_spent_today_ratio{account}`: the share of the week today may spend, and the share spent since the day began, 0 to 1. They read the same with `pacing.enabled` off

Work items:

- `kelpie_work_items{phase}` gauge, sent after each step from the state file
- `kelpie_work_items_finished_total{outcome}`: `merged`, `closed`, `dropped`, from the ledger's `finished` line
- `kelpie_review_rounds_total`, `kelpie_fix_turns_total`, `kelpie_rulings_total`, from the same line
- `kelpie_work_item_seconds_total{outcome}`, the wall time of finished items
- `kelpie_ci_failures_total{tests}`: each head of a work item's pull request the gate reads red, counted once per head. `tests` is `sandbox-only` when every failing test the failed runs' logs name is in a module `docs/agents/sandbox-red-tests.md` lists whole, `runnable` when one is not, is in a module the doc lists only in part (a `some:` entry, where kelpie cannot tell which of its tests a worker cannot run), is a test failure in a runner kelpie cannot read (a failed step whose name says it ran tests and whose output holds none of cargo's shapes), or the log was cut before its names, or more runs failed than kelpie reads logs of, `none` when no test failed (lint, docs, fmt, a build that never ran one). The list is of this crate's modules, so it is used only on the repo that doc describes, kelpie's own: on any other project every failing test counts `runnable`. Each failing name is judged against the cargo run it appeared in, since one log tail holds several. The names are read from the same logs a red run's prompt carries. A head whose logs kelpie could not read, or found none of, counts nothing, and a merge queue removal is not counted. `src/runner/ci_failure.rs`

Features: whether what a project turned on runs.

- `kelpie_feature_enabled{feature}` gauge (1 or 0) at open and each reread: `code_graph`, `browser`, `dev_server` (browser on and `main`'s launch file selects a configuration), `hooks`
- `kelpie_feature_runs_total{feature,outcome}`: `code_graph` index `ok`, `failed`, `skipped` (no binary), `ended` (call ended first). `src/runner/code_graph.rs`
- `browser`: the bridge opened for a turn `ok`, `failed`, `skipped` (tools not installed). `src/runner/browser.rs`
- `dev_server`: `ok` (answered within `READY_WITHIN`), `failed` (could not be planned or started, or silent), `skipped` (no launch file on `main`, or none configured). `src/runner/dev_server.rs`
- `hooks`: each hook the runner runs, `ok` (exit 0 or a deliberate exit 2) or `failed`; no label per hook or point. The hooks carry the recorder (`Hooks::counted_by`); a `kelpie guard` or `kelpie confine` process has none, so the worker's own `commit`, `push` and `command` hooks are not counted. `src/hooks.rs`
- `KelpieFeatureNeverWorks` only counts a feature while `kelpie_feature_enabled` is 1.

MCP servers, tool calls and requests are read from a call's transcript once it ends, and so are the reports of `shep kelpie usage`: `metrics-transcripts.md`.

The dog's series (leases, summons, the forge hold) are in `metrics-dog.md`.

## Alerts and dashboard

- `assets/prometheus/kelpie-rules.yml`: a feature failed or skipped since the runner started with no `ok` run (30m); a feature failing in over half its runs in 6h; an MCP server failing to connect in over half its calls in 6h; a plan's week over 0.9 used with over a day to its reset (15m); a role's calls `failed`, `timed-out` or `unreadable` in over a quarter of 6h's calls; a runner's series dropped.
- `assets/grafana/kelpie.json`: spend by role and model, tokens by type, calls by how they ended, work items by phase, features, MCP servers and tool calls, mean call time, the dog's leases held and waiting, mean lease wait, summons and the forge hold, cost and tokens per merged work item, cache-read share, review rounds and fix turns per finished item, plan use by account and window, request context sizes and cache writes by what came before.
- `docs/guide/metrics.md`: scrape config, the series, what each alert means, loading a backfill.

## Backfill

- `shep kelpie usage [<project>] --openmetrics` (`src/usage/openmetrics.rs`) prints the ledger's call and finished lines as OpenMetrics counters, each total stamped with its line's `at`, for `promtool tsdb create-blocks-from openmetrics`. The lines are replayed through an uncapped `Recorder`, so names and labels are the live ones, with `sheep` the project and no `id` or `fold`. Lines are taken oldest first, imported ones included, and the totals run from the ledger's first line, as a runner started then would send them. Two lines in one second leave the later total.

## Not built

- MCP servers and tool calls for Codex (#757) and goose (#784).
- Feature runs of the hooks `kelpie guard` and `kelpie confine` run inside a worker's sandbox: another process, with no recorder.
- Requests for Codex (#757) and goose (#784).
- Canary: a real issue a night through each harness with every feature on, checked from its transcript. #927
