# Budget

Pacing, leases, forge limits, time, usage, settings.

## Pacing

- Daily allowance = account's weekly remainder over whole days to reset, fixed at day start; once spent nothing new dispatched (5-hour window + board screening already pace). #15, `src/runner/pace.rs`
- At 50% of 5-hour window no turn starts until reset; read only before a dispatch or new turn. #15
- `add <issue>` skips daily allowance, not 5-hour window. #15
- `pacing.enabled` (on): off turns limits off but still reads usage (before a free reset). #67
- Unreadable usage holds (guessing pacer would spend the account it protects). #15
- Each account paces separately; new item paces on every account its agents spend. #129
- `budget.per_item_usd` ($10) parks item `over-budget` before a turn or round, never mid-call; yes doubles it. #492, `src/work_item/spend.rs`

## Dog, leases, forge limits

- Runners talk to kelpie through shep: triggers down, `channel.metric` up as running totals (metrics drop under backpressure). `src/sheep.rs`
- Runner asks dog's book for lease via `lease.<kind>.want.<epoch>`, epoch = its pid (dog reclaims a dead run's leases without racing the new one); bot window = `reviews` in `hours` from its file. `src/dog/mod.rs`
- Dog saves book to `<kelpie home>/dog/book.json` on every change (restart grants no second summon in a window); unreadable book starts CodeRabbit closed 1h. #19
- `cargo-test`: the one worker lease, 3 holders default, held while a connection to dog's `lease.sock` is open (nothing polls a pid; worker never reaches shep socket). #218, `src/dog/mod.rs`
- Workers build outside the lease, run `shep kelpie lease run cargo-test -- <command>` (exit 75 no lease, 127 no command), kelpie named by path (guard refuses program from a variable). #218
- GPU lease is qwen scripts' own lock; dog stays out; kelpie never wraps a command that takes it (nothing waits on a lock it holds). Decided 2026-09-26 on #5
- Triggers that only ask (`status`, `timings`, `drain`) never wake loop; board read at most every 60s (`BOARD_POLL`). #394
- Failing step backs off 15s to 10 min; rate-limit answer holds every forge call until reset (10 min for secondary limit, longer if hit again). #394
- One forge hold and pace per login, kept by the dog (`forge` ask on `lease.sock`, `src/dog/forge.rs`): any runner's used-up answer holds all; a runner that cannot reach the dog uses its own (`DoorPace`, `src/adapters/door_pace.rs`). Dog's copy is in memory, so a dog restart clears it. #580
- Pace: 60 calls a minute and 5 a second per login (`PER_MINUTE`, `AT_ONCE`, `src/ports/pace.rs`), margin under GitHub's documented secondary limits; each project gets cap / active projects and waits for its own oldest grant to leave. Queued calls wait (`Clock::wait`), never fail. #580
- Hold ends at the answer's `retry-after`, else its `x-ratelimit-reset`, else the 10-min fallback; same for `gh` and App calls (App rate limits are `ForgeError::Limited`; `PacedVoice` holds and paces them, worker-token mint waits the hold, no fixed 30s). #580
- A step that retries gets one attempt per step (close of merged issue; ruling-comment edit backs off like any failing step). #580

## Where the time goes

- Every second of an item lands in one of six phases (`worker`, `review`, `ci`, `ruling`, `merge`, `other`), totals add up. #141, #331
- Charged at each save to the prior phase (no step knows timings); stopped time = `other` (decided 2026-10-01); saves about every 30s. GPU wait counts as `review`. #141, `src/runner.rs`
- `history` keeps last 100 finished items for `timings [n]`. #141

## Where the usage goes

- Every model call is a line in `<kelpie home>/<project>/usage.jsonl`, append-only (hold kelpie's cost against control room). `src/usage.rs`, #347
- Each step is a JSON object in `<kelpie home>/<project>/steps.jsonl` (`at` first), append-only, which `watch` and `usage --import` read; the out log gets one plain line per event, never JSON. `src/runner/step_log.rs`, #491
- Units = `input + 2 x hour cache writes + 1.25 x five-minute writes + 0.1 x cache reads + 5 x output` (control room weights). #347
- Call never reaching a model gets no line; call cut by stop written from launch draft; finished item adds `finished` line. #347
- Unwritable ledger fails no call or step. `shep kelpie usage` compares units per merged PR against baselines kept only on maintainer's machine. #347

## Settings

- Read through shep client: runner sheep's `[app.dogs.kelpie]` table + kelpie's `[kelpie]` in `dogs.toml` (start needs no dog); missing required key stops runner by name. #114, #332, `src/settings.rs`
- Changes apply at next wake (polled, no second connection); `git.*`, `hooks`, gateways wait for restart; failing change keeps old settings. `src/runner/reread.rs`
- Every setting is read by something; removed key refused by name with replacement. Pre-table files not read, named in error. #332, `src/settings/removed.rs`
- `git`: `checkout`, `remote` (`origin`'s GitHub repo when absent), `merging`, `issues`. `src/settings/git.rs`
- `ci`: `block`, `fix_attempts` (-1); `concurrency`: `active_items` (1), `pending_rulings` (2). `src/settings/ci.rs`
- `budget.per_item_usd` ($10); `pacing.enabled` (on), `pacing.kickoff_hours` (8, display only). `src/settings/budget.rs`
- `agents`: implementers, reviewers, pm, issue writer, advisor, researcher, `fallback_after`. `src/settings/agents.rs`
- `rulings`: `decide` (`shadow`), `pm_alone`, `full_panel_paths`. `src/settings/rulings.rs`
- `worker`: `allowed_domains`, `guard_hooks`, `instructions_file`, `turn_timeout` (60), `compact_window` (200000). `src/runner/turn.rs`
- `skills`: per-step overrides; `retro` off (the PM's batch retro when named); `triage`, `spec`, `reset` driven by no step yet. `src/skills.rs`
- `hooks`, `e2e`. ADR 0009, `src/settings/hooks.rs`, `src/settings/e2e.rs`
- `[kelpie]`: `webhook`, `gpu_metrics_url`, `codex_home`, `gateways`, `leases.cargo-test` (3). `src/webhook.rs`

## Not built

- Hour-of-day pacing + utilization logging, to catch a peak-hour effect. `docs/design-log.md` Decided, not built
- GPU busy vs idle waits; need sampling and new phases. #251

## Watching

- `shep kelpie watch [<issue> | pm]` streams runner steps + session transcripts; reads files only (no runner call). Sessions found by id from `session-started` step, never guessed folders. Starts at end; `--back <n>` shows history. #408
