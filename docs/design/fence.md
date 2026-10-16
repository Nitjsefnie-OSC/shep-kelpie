# Fence

What a worker may touch, run and write, and what only kelpie does. The runner
loop and turns are in workers.md, agent files and harnesses in agents.md.


- Workers start `--setting-sources project` + kelpie-written settings file; maintainer's hooks, plugins, skills don't load (on goose, the call's plugins' skills are linked into its own `config/skills`, `skills.md`). #98, `src/profile.rs`
- Kelpie's guard in every worker's settings; `worker.guard_hooks` adds Claude Code hooks; an unresolvable hook stops the runner at start. #108, `src/runner/guard_hooks.rs`
- Project `hooks` run own commands at `publish`, `commit`, `push`, `handover`, `command`, `write`, `turn_start`, `turn_end`, notices on every harness (`guard_hooks` reach Claude Code alone). ADR 0009, #454, `src/hooks.rs`
- Workers run `bypassPermissions` over `main` ruleset + OS sandbox + `kelpie guard` denylist; no auto mode (classifier outages blocked work). `src/profile.rs`
- Sandbox and `kelpie confine` hold writes to worktree + build folder, hosts to GitHub, the model, `allowed_domains`. `src/profile.rs`
- Guard denies what only kelpie does (merge, ready, `review please`, `gh api`, pushes to `main`, credential reads); label or ready change kelpie didn't make parks the worker. A goose reviewer, advisor, retro: `--goose --read-only` (agents.md). `src/guard.rs`
- Agent files in worktree (`.claude`, `.mcp.json`, `.codex`, `.agents`, `.pi`, `.goose`) fenced, checked before each call; change parks `agent-files` (a review could plant a hook). #98
- Every call runs whole inside kelpie's OS sandbox (`Sandbox` port, `srt` 0.0.77), hooks and MCP included; Claude Code's own sandbox off (macOS can't nest). #128
- `[app.dogs.kelpie.code_graph] enabled` (off) gives `Work` turns and reviewers on every harness codebase-memory-mcp: `tools install` fetches a pinned release, `Runner::prepared` names the server in the call's reach and the call's thread (lock let go, a stop ends it) indexes the worktree into `<project>/code-graphs/<issue>` before each call (UI off, daemon in `<project>/cbm/<issue>`), Claude Code a per-call `--mcp-config` allowing its tools, pi in its home's `mcp.json`, Codex as `-c` overrides, goose as `--with-extension` flags (`skills.md`). Its sandbox writes and binds only there and its `/tmp` scratch, opening the folders above (#948). No binary or failed index: a logged skip and a `doctor` line. Its `install` never runs. #599, `src/runner/code_graph.rs`, `src/adapters/mcp.rs`
- Guard refuses `claude`, `codex`, `pi` as programs (a nested session runs without hooks). `src/guard.rs`
- Linux: srt skips `denyWrite` globs, so `fence::deny_write` also lists nested harness folders present at call start (`src/fence/nested.rs`); one made during a call is caught by the check against `main` after it. srt also refuses every Unix socket, so no worker reaches the dog's door and `cargo-test` runs unleased; `doctor` says so. #488
- Each harness declares run-code + login files; fence holds all, so no call plants or reads another harness's. #233
