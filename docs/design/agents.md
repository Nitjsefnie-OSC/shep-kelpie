# Agents

Agent files, harnesses, gateways and spend by role. The runner loop, fence and skills are in workers.md.

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
- Gateway (`kind`, `url`, `key_env`) is reached through the `ModelGateway` port (`src/ports/gateway.rs`: models, key check, optional lease), one adapter per `kind`, `paddock` (the default) and `openai` (any OpenAI-compatible gateway: models and key from `GET /v1/models`, no lease, so it may evict a model a turn runs on); a kind with no lease runs its turn at once, ceiling from its start. It queues and places models: agents unpaced, lockless; key unset in every call + child; every call (worker turn, reviewer session or round) holds a heartbeat lease, and a `turn` reason is the wait (`gpu_wait`, `waiting`); a lease paddock refuses to renew, or leaves unanswered past its ttl, fails the turn, and a drop releases at once; every chat call to a keyed gateway sends `x-paddock-priority: batch` (paddock serves no priority as interactive, kelpie runs unattended); a worker's pi chat calls also send `x-paddock-lease: <id>`, its own lease (a reviewer's name none), taken before its forwarder opens (without it paddock gives a request any kelpie lease's free turn, and the first-come order is lost). #352, #497, #500, #606, #607, `src/spawn.rs`
- Spend = tokens per role, dollars only where reported; unpriced calls counted (zero never reads as free). #129
