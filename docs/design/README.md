# Design

Kelpie: dog of the maintainer's shep shepherd; one runner per project takes ready issues off a board, agent workers build, PRs go through reviewers, CI, merge gate. Maintainer asked only via rulings.
Rules live in Rust code; a model is called only for work and judgement.

## Always

- Gates, locks, rate windows live in code; Claude only for work and judgement. ADR 0001
- Kelpie controls each agent's context growth. `src/runner/turn.rs`
- Naming: dog = adopted `kelpie` (lease dog); project = sheep (`runner <project>`); agent calls = lambs. One kelpie for all projects (bot windows, pacer are machine/account-wide). ADR 0001 to 0004
- Binary is the dog only with no args + `SHEP_DOG_NAME` and `SHEP_NAME` set; needs shepherd channel from `shep adopt`; names itself in handshake so shep never lists it `silent`. `src/sheep.rs`, shep-pm/shep#656
- Needs shep 0.15.0+ in 0.15 (a 0.14 shepherd drops notes); `upgrade` refuses a build its shepherd would refuse. shep-pm/shep#688, #319, #656, #914
- Home: `$SHEP_HOME/kelpie` (`KELPIE_HOME` overrides), shep plugin, folder per project (no project may take a shared name); dog files stay under `$SHEP_HOME`. #253
- The shepherd is reached through the `Dial` port (`src/shepherd/connection.rs`): `Socket` is shep's control socket, and tests dial `FakeShepherd`, which answers in memory and binds nothing. A test binds a real socket only to prove the socket itself; the sandbox-red list names those. #587
- Socket path over 103 bytes: runner refuses to start (macOS caps 104). `src/runner.rs`
- Home never moved from `~/.kelpie`: refuses to start naming each old file, exit 78 (in `stop_exit_codes`, no restart into refusal). #311, #345
- State file version 64 (`VERSION` in `src/state.rs`); each change one way, so rollback means finishing or dropping open items first. `src/state/removed.rs`
- Rust for all business code; Tauri GUI after MVP, until then run and watched from a Claude session. #5
- Never put budget numbers in agent prompts; never let a worker merge or post as the maintainer (see workers.md, github-app.md).

## Files

| file | covers | read when you touch |
|---|---|---|
| work.md | projects, board, adoption, slots, issue writer | `src/board.rs`, `src/issues.rs`, `src/runner/adopt.rs`, `src/runner/slots.rs`, `src/flock/verbs.rs`, `src/guard/issues.rs` |
| pm.md | the project manager, its board file | `src/runner/pm.rs`, `src/runner/pm/`, `src/runner/briefing.rs` |
| workers.md | runner loop, turns, fence | `src/runner/turn.rs`, `src/profile.rs`, `src/guard.rs`, `src/hooks.rs` |
| browser.md | a worker's browser outside its sandbox: the bridge, the filter, the tools | `src/bridge.rs`, `src/browse.rs`, `src/adapters/bridge.rs`, `src/runner/browser.rs`, `src/settings/browser.rs`, `src/tools/browser.rs` |
| browser-dev-server.md | the dev server kelpie starts for a worker's browser: launch file, sandbox, port, readiness | `src/runner/dev_server.rs`, `src/runner/dev_server/launch.rs`, `src/ports/dev_server.rs`, `src/adapters/dev_server.rs` |
| compaction.md | when a session compacts, `worker.compact_window` per harness, the `COMPACT` call | `src/runner/compact.rs`, `src/adapters/pi/compact.rs`, `src/adapters/codex/context.rs`, `src/adapters/goose/compact.rs` |
| skills.md | step skills, repo plugins, how each harness loads them | `src/skills.rs`, `src/skills/`, `src/adapters/skills.rs`, `src/runner/turn.rs` (`prepared`) |
| agents.md | agent files, harnesses, gateways | `src/agents/`, `src/adapters/`, `src/settings/agents.rs`, `src/spawn.rs` |
| handover.md | a runner handing its in-flight worker turns to a new image, `upgrade` | `src/sheep/handover.rs`, `src/runner/flight/carry.rs`, `src/adapters/process/adopt.rs`, `src/upgrade/handover.rs` |
| waits-and-spend.md | a turn waiting for its model or host, `usage`, spend by role | `src/runner/turn/unreachable.rs`, `src/adapters/unreachable.rs`, `src/adapters/codex.rs`, `src/usage.rs`, `src/settings/agents.rs` |
| review.md | review, bots, rulings | `src/runner/review/`, `src/runner/ruling.rs`, `src/webhook.rs` |
| local-review.md | local reviewers (`command`, `endpoint`), their lock, the rounds kept after a work item ends | `src/runner/review/keep.rs`, `src/settings/local_rounds.rs`, `src/adapters/local/`, `src/settings/local.rs` |
| merge.md | CI, catching up, merge, after the merge | `src/runner/gate.rs`, `src/runner/merge.rs`, `src/runner/close_merged.rs`, `src/runner/main_ci.rs` |
| github-app.md | App tokens, posting, answers, ruling panel | `src/runner/voice/`, `src/runner/panel/`, `src/panel/`, `src/guard/gh.rs`, `src/worker_folder.rs` |
| contributions.md | other people's pull requests: `contributions` settings, detection | `src/settings/contributions.rs`, `src/settings/reviewers.rs`, `src/runner/contributions.rs`, `src/state/contributions.rs` |
| contribution-security.md | a contribution's security review, slots and budget | `src/runner/contributions/`, `src/runner/flight/security.rs`, `src/worktree/contribution.rs` |
| contribution-pass.md | the reviewers' pass over a green contribution head, rotation, the record | `src/runner/contributions/pass.rs`, `src/worktree/contribution.rs`, `src/state/contributions.rs` |
| contribution-talk.md | what contributors say: hearing, the record, answering, the quiet clock | `src/runner/contributions/heard.rs`, `src/runner/contributions/clock.rs`, `src/runner/thread_replies.rs`, `src/state/contributions.rs` |
| contribution-release.md | asking a contributor for a release, a yes, thumbs up or silence, the waiting ruling | `src/runner/contributions/release.rs`, `src/runner/contributions/ruling.rs`, `src/state/contributions.rs` |
| contribution-merge.md | the merge ruling a contribution always gets, its yes, thanks and no | `src/runner/contributions/merge.rs`, `src/runner/contributions/ruling.rs`, `src/settings/rulings.rs`, `src/state/contributions.rs` |
| representative.md | the representative: its agent file, `agents.representative`, the one call that words a post, the mention check | `src/runner/representative.rs`, `src/agents/front.rs`, `src/settings/agents.rs`, `agents/representative.md` |
| dependencies.md | the bumps bots open: `dependencies` settings, rebase asks, proof, merge, red | `src/settings/dependencies.rs`, `src/runner/dependencies.rs`, `src/runner/flight/proving.rs`, `src/runner/dev_server/bump.rs`, `src/state/bumps.rs`, `src/adapters/e2e_runner.rs`, `src/adapters/pages.rs`, `src/adapters/bridge/client.rs` |
| triage.md | settings, the triager, the pick rule, `triage.since`, the verb, `status` | `src/settings/triage.rs`, `src/runner/triage.rs`, `src/runner/open_checks.rs`, `src/ports/issue.rs`, `agents/triager.md` |
| triage-pass.md | one issue's pass: post, labels, the duplicate close, agent-ready | `src/runner/triage/`, `src/runner/flight/`, `src/ports/voice.rs` |
| log-lines.md | what the runner and the dog log between steps: pause, handover, lease waits, board skips, standing warnings | `src/runner/drain.rs`, `src/runner/skip_log.rs`, `src/runner/review_bot/lease_wait.rs`, `src/runner/claim.rs`, `src/dog/desk.rs`, `src/dog/door.rs` |
| metrics.md | kelpie's series on shep's metrics endpoint, the alerts and dashboard | `src/metrics.rs`, `src/metrics/`, `src/ports/metrics.rs`, `src/adapters/metric_sink.rs`, `assets/prometheus/`, `assets/grafana/` |
| metrics-dog.md | the dog's series: leases, summons and the forge hold | `src/metrics/dog.rs`, `src/dog/watch.rs` |
| metrics-transcripts.md | the series and reports read from a call's transcript: MCP servers, tool calls, requests, `shep kelpie usage` | `src/mcp_use.rs`, `src/request_use.rs`, `src/runner/transcript_count.rs`, `src/usage/` |
| budget.md | pacing, dog + leases, time, usage, settings | `src/runner/pace.rs`, `src/dog/`, `src/usage.rs`, `src/settings/`, `src/sheep.rs` |

Each file stays under 8 KB; a changed decision updates its bullet in the same PR. An area past it splits in two, with a new row here. `scripts/design-size.sh` checks both, in CI too.

## Elsewhere

- `docs/design-log.md`: full record, mechanics, history, old file formats
- `docs/facts/`: measurements
- `docs/adr/`: hard-to-reverse decisions
- `GLOSSARY.md`: vocabulary
