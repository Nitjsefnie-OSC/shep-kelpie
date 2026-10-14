# Workers

Runner loop, worker turns, fences, agents and harnesses. Skills and plugins are in skills.md.

## Runner and turns

- Loop never waits on an agent call: each runs on its own thread, one per item, beside the PM's (triggers and `status` answer meanwhile). #318, ADR 0005, `src/runner/flight.rs`
- Harness files carry the issue (`settings-<issue>.json`); runner git waits 2s for a ref lock, 5s for `packed-refs`. `src/runner/turn.rs`
- On stop: SIGTERM each call's process group, save, exit inside shep's 1.6s `kill_timeout`; restart resumes each item from its session. A `handover` trigger instead keeps the calls running (agents.md). ADR 0005
- Worker owns one item, builds inline, no plan or crew; instructions and headless rules forbid sub-agents. ADR 0006
- Compaction: session whose last turn passed 150k context compacts before next turn (`WORKER_COMPACT_PAST`); Claude Code compacts mid-turn at `worker.compact_window` (200000). A pi session is compacted past 150k the same way: its reply's context is the last request's prompt, and a `COMPACT` call sends the `/kelpie-compact` command of an extension kelpie writes (`-p` runs no built-in command). pi's own threshold compaction still runs, usage counted; on a window under 150k only the harness's own runs. #760 Codex compacts at the window; `COMPACT` lowers the next turn's limit under its context. `codex/context.rs`. #770
- `worker.compact_window` on pi: `settings.json` in the call's pi home sets `compaction.reserveTokens` to the model's `context` less the window, only when above pi's default 16384 (else no file). The extension hands the turn's prompt back (`<settings>.prompt.md`, `sendMessage` steered) after a `threshold` or `overflow` compaction, not a `manual` one, which gets the runner's reminder. `src/adapters/pi/compact.rs`. #761
- Agents never see budget numbers (it makes models stop after each task). `src/runner/turn/budget.rs`
- Implementers default `sonnet-high`, `opus-high` for work hard to undo. #313
- `worker.turn_timeout` (60 min) counts from turn start, so restart buys no hour; past it parks `turn-timeout`; lease wait refunded. #16, `src/runner/turn/unfinished.rs`
- Failed turn parks `stuck` `turn-failed`. #57
- Pause ends a Claude Code or pi worker turn at its next tool result, not its end: runner writes `kelpie-pause` in the item's build folder (the one folder the guard can read); `kelpie guard --pause=<build>` as `PostToolUse` answers `{"continue":false}`, pi's guard extension calls `ctx.abort()`, each leaving `kelpie-paused`, which tells a paused turn from any other. No unpushed check, review or ruling, same stage, no slot or lease; next start resumes with a fresh ceiling. Else a Codex or goose call keeps the old pause. #821, `src/guard/pause.rs`, `src/runner/turn/paused.rs`
- A due turn that does not start says why in `status` (`held_up`) and on the board line: no slot, push by someone else, pacer hold, token wait, drain, `attach`. Memory only; one log line once held 5 min. #503, `src/runner/turn/held_up.rs`
- First turn ending with no PR or question resumed once, then parks; uncommitted work with no push sent back once to commit. #140, #275
- Item whose issue closed with no work ends with no ruling; a parked item's issue is read every 5 min. #385
- Retro moved from worker to PM: batch after every 10 finished items and at a drain, one call with the PM's agent over a digest (`journal.jsonl`, rulings, ledger). Overnight 30 retros were ~14% of spend (~94k-token cache write for ~3k of report). Worker keeps a PR-body `Friction` section. Off unless `skills.retro`; no slot, never retried, saved unread (agents acting on retros loop). Digest counts conflicts per file, rounds, late findings, known flakes (also in CI fix prompts). #361, #566, #657, `src/runner/retro.rs`
- Worker asks with a `<kelpie-question>` block ending its message; parks on a `question` ruling with optional `- ` choices. #13
- A choice only the maintainer can carry out (run a command, change a setting, grant a permission) starts with `You`, names what they do and what the worker does after, in the worker's instructions and the PM's `candidates` and `escalate`: an answer is the worker's next turn, so a bare verb reads as an order to a worker that may be unable to obey (shep#829). Neither prompt offers more worker GitHub access as a choice; the token's scope is a design decision (`docs/design/github-app.md`). #805, `src/worker-instructions.md`, `src/runner/pm/prompt.rs`
- `shep kelpie attach <issue>` hands a worker's Claude Code or pi session to maintainer, sandboxed; held by pid + start time; pi asks nothing. #325, #766
- PR template filled, ending `Resolves #<issue>`; no repo template means the owner's `.github` one, read at start and reread, cut past 8 KiB. #596; `worker.instructions_file` adds project rules. #87, `src/runner/instructions.rs`
- Out-of-scope finds named in final message; deferred review findings in the deferred findings file the instructions name; PR stays on its issue. #649, `src/runner/follow_up.rs`

## Fence

- Workers start `--setting-sources project` + kelpie-written settings file; maintainer's hooks, plugins, skills don't load. #98, `src/profile.rs`
- Kelpie's guard in every worker's settings; `worker.guard_hooks` adds Claude Code hooks; an unresolvable hook stops the runner at start. #108, `src/runner/guard_hooks.rs`
- Project `hooks` run own commands at `publish`, `commit`, `push`, `handover`, `command`, `write`, `turn_start`, `turn_end`, notices on every harness (`guard_hooks` reach Claude Code alone). ADR 0009, #454, `src/hooks.rs`
- Workers run `bypassPermissions` over `main` ruleset + OS sandbox + `kelpie guard` denylist; no auto mode (classifier outages blocked work). `src/profile.rs`
- Sandbox and `kelpie confine` hold writes to worktree + build folder, hosts to GitHub, the model, `allowed_domains`. `src/profile.rs`
- Guard denies what only kelpie does (merge, ready, `review please`, `gh api`, pushes to `main`, credential reads); label or ready change kelpie didn't make parks the worker. `src/guard.rs`
- Agent files in worktree (`.claude`, `.mcp.json`, `.codex`, `.agents`, `.pi`, `.goose`) fenced, checked before each call; change parks `agent-files` (a review could plant an unsandboxed hook). #98
- Every call runs whole inside kelpie's OS sandbox (`Sandbox` port, `srt` 0.0.77), hooks and MCP included; Claude Code's own sandbox off (macOS can't nest). #128
- `[app.dogs.kelpie.code_graph] enabled` (off) gives the worker's `Work` turns and the reviewers, on Claude Code, pi or Codex, codebase-memory-mcp: `tools install` fetches a release pinned by SHA-256, `Runner::prepared` names the server in the call's reach and the call's own thread (lock let go, a stop ends it) indexes the worktree into `<project>/code-graphs/<issue>` before each call, Claude Code gets a per-call `--mcp-config` (`--strict-mcp-config` stays) allowing `mcp__codebase-memory-mcp`, pi in its home's `mcp.json`, Codex as `-c mcp_servers.<name>` overrides (`skills.md`), and the sandbox lets the server write only its index. No binary or a failed index: a logged skip and a `doctor` line. Its `install` never runs. #599, `src/runner/code_graph.rs`, `src/adapters/mcp.rs`
- Guard refuses `claude`, `codex`, `pi` as programs (a nested session runs without hooks). `src/guard.rs`
- Linux: srt skips `denyWrite` globs, so `fence::deny_write` also lists nested harness folders present at call start (`src/fence/nested.rs`); one made during a call is caught by the check against `main` after it. srt also refuses every Unix socket, so no worker reaches the dog's door and `cargo-test` runs unleased; `doctor` says so. #488
- Each harness declares run-code + login files; fence holds all, so no call plants or reads another harness's. #233

## Not built

- Claude Code through a gateway: needs per-harness forwarding + a pacing decision. Gateways go through a port (`src/ports/gateway.rs`) with paddock and `openai` adapters; `openai` holds no lease and may evict a turn's model. #352
- Usage calculator handing workers decisions, never cost figures. `docs/design-log.md` Decided, not built
- Shots and preview: in git history. ADR 0006, #54, #131
