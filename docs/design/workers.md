# Workers

Runner loop, worker turns, fences, skills, agents and harnesses.

## Runner and turns

- Loop never waits on an agent call: each runs on its own thread, one per item, beside the PM's (triggers and `status` answer meanwhile). #318, ADR 0005, `src/runner/flight.rs`
- Harness files carry the issue (`settings-<issue>.json`); runner git waits 2s for a ref lock, 5s for `packed-refs` (items share the repo). `src/runner/turn.rs`
- On stop: SIGTERM each call's process group, save, exit inside shep's 1.6s `kill_timeout`; restart resumes each item from its session. One group per call lets kelpie end one call at its ceiling. ADR 0005
- Worker owns one item, builds inline, no plan, no crew; instructions and headless rules forbid sub-agents. ADR 0006
- Compaction: session whose last turn passed 150k context compacts before next turn (`WORKER_COMPACT_PAST`); Claude Code compacts mid-turn at `worker.compact_window` (200000). #426, #476
- Agents never see budget numbers (budget talk makes models stop after each task). `src/runner/turn/budget.rs`
- Implementers default `sonnet-high`, `opus-high` for work hard to undo (Sonnet high builds as well as Opus; `xhigh` no better). #313
- `worker.turn_timeout` (60 min) counts from turn start, so restart buys no hour; past it parks `turn-timeout`; lease wait refunded. #16, `src/runner/turn/unfinished.rs`
- Failed turn parks `stuck` `turn-failed`. #57
- A due turn that does not start says why in `status` (`held_up`) and on the board line: no slot, push by someone else, pacer hold, GitHub token wait, drain, `attach`. Memory only; one log line once held 5 min, once per reason. #503, `src/runner/turn/held_up.rs`
- First turn ending with no PR or question resumed once, then parks; uncommitted work with no push sent back once to commit. #140, #275
- Item whose issue closed with no work ends with no ruling; parked item's issue read at most every 5 min. #385
- Retro moved from worker to PM: batch after every 10 finished items and at a drain, one call with the PM's agent over a digest kelpie builds (`journal.jsonl`, rulings, ledger). The worker never saw PM wakes, rulings or merges, and overnight 30 retros were ~14% of spend (~94k-token cache write for ~3k of report). Worker keeps a PR-body `Friction` section. Off unless `skills.retro`; no slot, never retried, saved unread (agents acting on retros loop). Prompt carries the build and open known gaps (themes of unconsolidated reports), asks for new items; digest counts conflicts per file, rounds, late findings, known flakes (also in CI fix prompts). #361, #566, #657, `src/runner/retro.rs`
- Worker asks with a `<kelpie-question>` block ending its message; parks on `question` ruling with optional `- ` choices. #13
- `shep kelpie attach <issue>` hands a Claude Code worker's session to maintainer without `bypassPermissions`; held by pid + start time (reused pid holds nothing). #325
- PR template filled, ending `Resolves #<issue>`; no repo template means the owner's `.github` one, read at start and reread, text in the instructions, cut past 8 KiB. #596; `worker.instructions_file` adds project rules. #87, `src/runner/instructions.rs`
- Out-of-scope finds named in final message; deferred review findings in deferred findings file, whose path the instructions name; PR stays on its issue. #649, `src/runner/follow_up.rs`

## Fence

- Workers start `--setting-sources project` + kelpie-written settings file; maintainer's hooks, plugins, skills don't load. #98, `src/profile.rs`
- Kelpie's guard in every worker's settings; `worker.guard_hooks` adds Claude Code hooks; unresolvable hook stops runner at start. #108, `src/runner/guard_hooks.rs`
- Project `hooks` run own commands at `publish`, `commit`, `push`, `handover`, `command`, `write`, `turn_start`, `turn_end`, notices on every harness (`guard_hooks` reach Claude Code alone). ADR 0009, #454, `src/hooks.rs`
- Workers run `bypassPermissions` over `main` ruleset + kelpie OS sandbox + `kelpie guard` denylist; no auto mode (classifier outages blocked work). `src/profile.rs`
- Sandbox and `kelpie confine` hold writes to worktree + build folder, hosts to GitHub, the model, `worker.allowed_domains`. `src/profile.rs`
- Guard denies what only kelpie does (merge, ready, `review please`, `gh api`, pushes to `main`, credential reads); label or ready change kelpie didn't make parks the worker. `src/guard.rs`
- Agent files in worktree (`.claude`, `.mcp.json`, `.codex`, `.agents`) fenced and checked before each call; change parks `agent-files` (a review could plant an unsandboxed hook). #98
- Every call runs whole inside kelpie's OS sandbox (`Sandbox` port, `srt` 0.0.77), hooks and MCP included; Claude Code's own sandbox off (macOS can't nest). #128
- `[app.dogs.kelpie.code_graph] enabled` (off) gives the worker's `Work` turns and Claude Code reviewers codebase-memory-mcp: `tools install` fetches a release pinned by SHA-256, `Runner::prepared` names the server in the call's reach and the call's own thread (lock let go, a stop ends it) indexes the worktree into `<project>/code-graphs/<issue>` before each call, the adapter writes a per-call `--mcp-config` (`--strict-mcp-config` stays) and allows `mcp__codebase-memory-mcp`, and the sandbox lets the server write only its index. Missing binary or failed index: a logged skip and a `doctor` line, never a crash. The tool's own `install` is never run. #599, `src/runner/code_graph.rs`, `src/adapters/claude/graph.rs`
- Guard refuses `claude`, `codex`, `pi` as programs (model endpoint open to process; nested session runs without hooks). `src/guard.rs`
- Linux: srt skips `denyWrite` globs, so `fence::deny_write` also lists nested harness folders present at call start (bounded walk, `src/fence/nested.rs`); one made during a call is caught by the check against `main` after it. srt also refuses every Unix socket there, so no worker reaches the dog's door and `cargo-test` runs unleased; `doctor` says so. #488
- Each harness declares run-code + login files; fence holds all, so one harness's call can't plant or read another's. #233

## Skills

- Every step runs a skill: mattpocock/skills vendored + pinned in `skills/`, kelpie prompts as fallback; `[skills]` overrides a step. #147, `src/skills.rs`
- Skills byte-identical; kelpie headless rules win (decide and record, no sub-agents, kelpie reply format, no own `/code-review`). Skill that can't load runs kelpie prompt, logged (unknown slash command passes as text). #170, #147
- Plugins enabled in `.claude/settings.json` on `origin/main` reach every agent (skills, agents, commands and hooks; MCP is slice 5), never from a worker branch. Sources: plain git transports, no `-` or `::` addresses, no links or `..` (default branch settings aren't maintainer's alone). #569, #570
- The repo's own `.claude/skills`, `agents`, `commands` and the `hooks` of `.claude/settings.json`, read from `origin/main`, are one more plugin, `kelpie-repo`, for every call; a Claude Code call in a worktree whose agent files equal `main`'s (`fence::matches_main`) loads them there and skips it. `repo_setup.own = false` leaves it out. #591, `src/skills/repo/own.rs`
- `[app.dogs.kelpie.repo_setup]` narrows them: `plugins = false` drops the repo's, `roles` picks the calls that get any (applied in `Runner::prepared`), `skip` drops `name@marketplace` ids, `parts` picks which of a plugin's parts load (a plugin's hooks stay in its own `hooks/hooks.json`, beside the call's settings file's hooks, never in it, so they cannot replace kelpie's), `extra` adds ids with a one-string source (`github:o/n#ref`, `git:url`, `directory:dir`) that lookout can edit. The CLI's headless issue writer isn't the runner, so it loads them itself into a per-run folder. #571

## Not built

- Claude Code through a gateway: needs per-harness forwarding + pacing decision. #352
- Usage calculator handing workers decisions, never cost figures (budget talk makes models stop). `docs/design-log.md` Decided, not built
- Shots and preview parked, recoverable from git history. ADR 0006, #54, #131
