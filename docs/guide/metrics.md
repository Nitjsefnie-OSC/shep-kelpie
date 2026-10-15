# Metrics

Each project's runner sends its numbers to shep's metrics dog, which serves them on its Prometheus endpoint beside the flock's own series. You can watch spend, calls, work items and whether the features you turned on actually work, live, and alert on them.

## Turning it on

```sh
shep enable metrics          # serves http://127.0.0.1:9615/metrics
curl -s 127.0.0.1:9615/metrics | grep '^kelpie_'
```

The runners send to it already: there is nothing to set in kelpie. A runner sends every series again once a minute, so a metrics dog enabled or restarted after the runners has them all within a minute.

Point Prometheus at it, and load kelpie's alert rules from `assets/prometheus/kelpie-rules.yml`:

```yaml
global:
  scrape_interval: 15s
rule_files:
  - kelpie-rules.yml
scrape_configs:
  - job_name: shep
    static_configs:
      - targets: ["127.0.0.1:9615"]
```

Import `assets/grafana/kelpie.json` into Grafana for the dashboard. Shep's own dashboard, for the flock and the host, is in shep's `assets/grafana/`.

## The series

Every series carries `sheep`, the project's name, as well as `id` and `fold`, which shep's metrics dog adds.

| Series | Labels | What it counts |
|---|---|---|
| `kelpie_calls_total` | `role`, `kind`, `harness`, `ended` | Model calls that ended, and how: `answered`, `unreadable`, `failed`, `timed-out`, `stopped`, `paused` |
| `kelpie_tokens_total` | `role`, `harness`, `model`, `type` | Tokens as the harness reported them: `input`, `output`, `cache_read`, `cache_write` |
| `kelpie_cost_usd_total` | `role`, `harness`, `model` | US dollars, from calls whose harness reported a cost |
| `kelpie_unpriced_calls_total` | `role`, `harness` | Calls whose harness reported no cost |
| `kelpie_call_seconds_total` | `role`, `kind` | Seconds the calls ran; over `kelpie_calls_total` for a mean |
| `kelpie_work_items` | `phase` | Open work items: `implement`, `review`, `ci`, `ruling`, `merge`, `done` |
| `kelpie_work_items_finished_total` | `outcome` | Work items finished: `merged`, `closed` (no change), `dropped` |
| `kelpie_work_item_seconds_total` | `outcome` | Wall seconds of the finished work items |
| `kelpie_review_rounds_total`, `kelpie_fix_turns_total`, `kelpie_rulings_total` | | Review rounds, fix turns and rulings of the finished work items |
| `kelpie_feature_enabled` | `feature` | 1 when the project's settings turn the feature on: `code_graph`, `browser`, `hooks` |
| `kelpie_feature_runs_total` | `feature`, `outcome` | Times a feature ran for a call. The code graph's index: `ok`, `failed`, `skipped` (no server installed), `ended` (the call ended first) |
| `kelpie_mcp_servers_total` | `server`, `outcome` | MCP servers given to calls, read from each call's transcript once it ends: `connected` or `failed`. `server` is `codebase-memory-mcp` or `playwright` for kelpie's own, and `repo` for any your project names. Claude Code calls only: pi does not record a server that failed |
| `kelpie_mcp_tool_calls_total` | `server` | Tool calls a call made to each server, from Claude Code's and pi's transcripts. A server given and never called reads 0 |
| `kelpie_series_dropped_total` | | Samples of a series the runner held back, past the 200 it sends |

A runner's counters start at zero when it starts, as Prometheus expects of a restart: read them with `rate` or `increase`. When a runner stops, its series go from the endpoint until it starts again.

## The alerts

- **KelpieFeatureNeverWorks**: a feature has failed or been skipped since the runner started and has never worked once. Its calls run without it. The runner's log says why, in a line with `runs without`.
- **KelpieFeatureFailing**: a feature failed in over half its runs in the last 6 hours.
- **KelpieMcpServerFailing**: an MCP server failed to connect in over half the calls given it in the last 6 hours, so those calls ran without its tools. The runner's log names the server, the call and the error code, in a line with `failed to connect`, and `shep kelpie status` lists it under `standing` until a call finds it connected.
- **KelpieCallsFailing**: over a quarter of a role's calls in 6 hours failed, timed out or came back unreadable.
- **KelpieSeriesDropped**: a runner holds back series past its cap, so some panels miss data. Its log names the series.

## By work item

The endpoint has no work item, pull request or session in its labels, so its series stay few. To see one work item's calls and cost, use `shep kelpie usage`, which reads the project's usage ledger (see [Commands](commands.md)). Both are written from the same call lines, so they agree.

`shep kelpie issue` runs the issue writer outside the runner, so its calls are in the ledger but not on the endpoint.
