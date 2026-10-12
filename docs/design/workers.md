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
- First turn ending with no PR or question resumed once, then parks; uncommitted work with no push sent back once to commit. #140, #275
- Item whose issue closed with no work ends with no ruling; parked item's issue read at most every 5 min. #385
- Retros off unless `skills.retro`; no slot, never retried, saved unread (agents acting on retros loop). #361, `src/runner/retro.rs`
- Worker asks with a `<kelpie-question>` block ending its message; parks on `question` ruling with optional `- ` choices. #13
- `shep kelpie attach <issue>` hands a Claude Code worker's session to maintainer without `bypassPermissions`; held by pid + start time (reused pid holds nothing). #325
- PR template filled, ending `Resolves #<issue>`; `worker.instructions_file` adds project rules. #87, `src/runner/instructions.rs`
- Out-of-scope finds named in final message; deferred review findings in deferred findings file; PR stays on its issue. `src/runner/follow_up.rs`

## Fence

- Workers start `--setting-sources project` + kelpie-written settings file; maintainer's hooks, plugins, skills don't load. #98, `src/profile.rs`
- Kelpie's guard in every worker's settings; `worker.guard_hooks` adds Claude Code hooks; unresolvable hook stops runner at start. #108, `src/runner/guard_hooks.rs`
- Project `hooks` run own commands at `publish`, `commit`, `push`, `handover`, `command`, `write`, `turn_start`, `turn_end`, notices on every harness (`guard_hooks` reach Claude Code alone). ADR 0009, #454, `src/hooks.rs`
- Workers run `bypassPermissions` over `main` ruleset + kelpie OS sandbox + `kelpie guard` denylist; no auto mode (classifier outages blocked work). `src/profile.rs`
- Sandbox and `kelpie confine` hold writes to worktree + build folder, hosts to GitHub, the model, `worker.allowed_domains`. `src/profile.rs`
- Guard denies what only kelpie does (merge, ready, `review please`, `gh api`, pushes to `main`, credential reads); label or ready change kelpie didn't make parks the worker. `src/guard.rs`
- Agent files in worktree (`.claude`, `.mcp.json`, `.codex`, `.agents`) fenced and checked before each call; change parks `agent-files` (a review could plant an unsandboxed hook). #98
- Every call runs whole inside kelpie's OS sandbox (`Sandbox` port, `srt` 0.0.77), hooks and MCP included; Claude Code's own sandbox off (macOS can't nest). #128
- Guard refuses `claude`, `codex`, `pi` as programs (model endpoint open to process; nested session runs without hooks). `src/guard.rs`
- Each harness declares run-code + login files; fence holds all, so one harness's call can't plant or read another's. #233

## Skills

- Every step runs a skill: mattpocock/skills vendored + pinned in `skills/`, kelpie prompts as fallback; `[skills]` overrides a step. #147, `src/skills.rs`
- Skills byte-identical; kelpie headless rules win (decide and record, no sub-agents, kelpie reply format, no own `/code-review`). Skill that can't load runs kelpie prompt, logged (unknown slash command passes as text). #170, #147
- Plugins enabled in `.claude/settings.json` on `origin/main` reach every agent (skills only for now), never from a worker branch. Sources: plain git transports, no `-` or `::` addresses, no links or `..` (default branch settings aren't maintainer's alone). #569, #570

## Agents

- Agent = `<name>.md`, YAML frontmatter tagged `role` + Markdown body; bad file stops runner naming file + line. serde-saphyr (serde_yaml archived). ADR 0006, #313
- Embedded from `agents/`; home file overrides, `add` writes missing, reread each wake (edits land live). Item keeps its agent, runs the file as it is now. #313
- `extends` merges frontmatter keys as text before parsing (every check applies); parameters on a reference refused (each a new label + ledger key). #386, `src/agents/extends.rs`
- `agents.implementers` (default `["sonnet-high"]`, first is default), `reviewers`, `pm`, `issue_writer`; missing or wrong-role file stops runner. `src/settings/agents.rs`
- One agent interface for every role, harness details in adapters; harness that can't fence can't be worker. ADR 0006
- pi: second harness; kelpie extension runs `confine` + `guard`; forwarder passes only chat completions. No Claude Code hooks, so `worker.guard_hooks` naming pi or Codex stops at load. #130, #235, `src/settings/agents.rs`
- Codex: third harness (ChatGPT plan windows beside Claude's, each paced on own); session ids map to Codex threads. Own login in `codex_home`, fresh `CODEX_HOME` per call (`~/.codex` copied maintainer's history). #233, `src/adapters/codex.rs`
- Codex writes only an allow-list of its home (`.env`, `AGENTS.override.md` could disable guards); only Codex workers get a shell (reviewed PR could steer it); output to files (shared pipe killed it on large output). `src/adapters/codex.rs`
- `e2e.host` ssh alias, must be disposable (workers edit the script); worker asks only `take: "e2e"`; dog runs one per host under 30 min. #415, #521, `src/dog/e2e.rs`
- Local implementer (`usage: none`) can be default; without a gateway keep `active_items` 1. #381
- Turn awaiting its model marked `waiting` in memory; with `agents.fallback_after`, waiting first turn moves to next free implementer; later or pinned turns never (lose session). #381, `src/runner/flight/waiting.rs`
- Unreachable pi host retried 30s to 5 min for 30 min before parking. #457, `src/runner/turn/unreachable.rs`
- `usage` (`claude`, `codex`, `none`) must match harness; `none` never paced, limited by `lease` (`gpu`) or gateway. #129
- Gateway (`url`, `key_env`) queues and places models: agents unpaced, lockless; key unset in every call + child; worker turn holds heartbeat lease; a lease paddock refuses to renew, or leaves unanswered past its ttl, fails the turn, and a drop releases at once. #352, #497, `src/spawn.rs`
- Spend = tokens per role, dollars only where reported; unpriced calls counted (zero never reads as free). #129

## Not built

- Claude Code through a gateway: needs per-harness forwarding + pacing decision. #352
- Usage calculator handing workers decisions, never cost figures (budget talk makes models stop). `docs/design-log.md` Decided, not built
- Shots and preview parked, recoverable from git history. ADR 0006, #54, #131
