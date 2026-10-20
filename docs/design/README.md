# Design

Kelpie: dog of the maintainer's shepherd; one runner per project takes ready issues off a board, workers build, PRs go through reviewers, CI, merge gate. Maintainer asked only via rulings.

## Always

- Gates, locks, rate windows live in code; Claude only for work and judgement. ADR 0001
- Kelpie controls each agent's context growth. `src/runner/turn.rs`
- Naming: dog = adopted `kelpie` (lease dog); project = sheep; agent calls = lambs. One kelpie for all projects (bot windows, pacer are machine/account-wide). ADR 0001 to 0004
- Binary is the dog with no args + `SHEP_DOG_NAME` and `SHEP_NAME` set; needs the `shep adopt` channel; names itself in handshake so shep never lists it `silent`. `src/sheep.rs`, shep-pm/shep#656
- Needs shep 0.15.0+ in 0.15; `upgrade` refuses a build its shepherd would. shep-pm/shep#688, #319, #656, #914
- Home: `$SHEP_HOME/kelpie` (`KELPIE_HOME`), shep plugin, folder per project; dog files stay under `$SHEP_HOME`. #253
- The shepherd is reached through the `Dial` port (`crates/core/src/shepherd/connection.rs`): `Socket` is shep's control socket; tests dial `FakeShepherd`, which answers in memory. #587
- Socket path over 103 bytes: runner refuses to start (macOS caps 104). `src/runner.rs`
- Home never moved from `~/.kelpie`: refuses to start naming each old file, exit 78 (in `stop_exit_codes`, no restart into it). #311, #345
- State file version 72 (`VERSION` in `crates/core/src/state.rs`); each change one way: rollback means finishing or dropping open items first. `crates/core/src/state/removed.rs`
- Business code is Rust; Tauri GUI after MVP, until then run and watched from a Claude session. #5
- Crates under `crates/` use nothing above them. ADR 0015
- Never put budget numbers in agent prompts; never let a worker merge or post as the maintainer (see fence.md, github-app.md).

## Files

| file | covers | read when you touch (`core/` is `crates/core/src/`, `dog/` is `crates/dog/src/dog/`) |
|---|---|---|
| work.md | projects, board, adoption, slots, issue writer | `core/board.rs`, `src/issues.rs`, `src/runner/adopt.rs`, `src/runner/slots.rs`, `src/flock/verbs.rs`, `core/guard/issues.rs` |
| pm.md | the project manager, its board file | `src/runner/pm.rs`, `src/runner/pm/`, `src/runner/briefing.rs` |
| workers.md | runner loop, turns | `src/runner/turn.rs`, `src/runner/flight.rs` |
| fence.md | sandbox, guard, hooks, reads | `core/{profile,guard,hooks,dep_sources}.rs`, `src/runner/turn/dep_reads.rs` |
| browser.md | a worker's browser outside its sandbox: bridge, filter, tools | `core/{bridge,browse}.rs`, `src/adapters/bridge.rs`, `src/runner/browser.rs`, `core/settings/browser.rs`, `crates/process/src/tools/` |
| browser-dev-server.md | the dev server for a worker's browser: launch, port, readiness | `src/runner/dev_server.rs`, `src/runner/dev_server/launch.rs`, `core/ports/dev_server.rs`, `src/adapters/dev_server.rs` |
| compaction.md | when a session compacts, `worker.compact_window` per harness, `COMPACT` | `src/runner/compact.rs`, `src/adapters/pi/compact.rs`, `src/adapters/codex/context.rs`, `src/adapters/goose/compact.rs` |
| skills.md | step skills, repo plugins, how harnesses load them | `core/skills.rs`, `core/skills/`, `src/adapters/skills.rs`, `src/runner/turn.rs` (`prepared`) |
| agents.md | agent files, harnesses, gateways | `core/agents/`, `src/adapters/`, `core/settings/agents.rs`, `crates/process/src/spawn.rs` |
| handover.md | a runner handing in-flight turns to a new image, `upgrade` | `src/sheep/handover.rs`, `src/runner/flight/carry.rs`, `src/adapters/process/adopt.rs`, `src/upgrade/handover.rs` |
| waits-and-spend.md | a turn waiting for its model or host, spend by role | `src/runner/turn/unreachable.rs`, `src/adapters/unreachable.rs`, `src/adapters/codex.rs`, `core/usage.rs`, `core/settings/agents.rs` |
| review.md | review, bots, rulings | `src/runner/review/`, `src/runner/ruling.rs`, `core/webhook.rs` |
| local-review.md | local reviewers (`command`, `endpoint`), their lock, rounds kept after an item | `src/runner/review/keep.rs`, `core/settings/local_rounds.rs`, `core/settings/local.rs`, `src/adapters/local/` |
| merge.md | CI, catching up, merge, after it | `src/runner/gate.rs`, `src/runner/merge.rs`, `src/runner/close_merged.rs`, `src/runner/main_ci.rs` |
| local-tickets.md | the local ticket store, `ticket` verbs, closing on merge | `core/local_tickets.rs`, `src/flock/ticket.rs`, `src/runner/close_merged.rs` |
| github-app.md | App tokens, posting, answers, ruling panel | `src/runner/voice/`, `src/runner/panel/`, `crates/panel/`, `core/guard/gh.rs`, `crates/worker_folder/` |
| contributions.md | other people's pull requests: `contributions` settings, detection | `core/settings/{contributions,reviewers}.rs`, `src/runner/contributions.rs`, `core/state/contributions.rs` |
| contribution-security.md | a contribution's security review, slots, budget | `src/runner/contributions/`, `src/runner/flight/security.rs`, `crates/process/src/worktree/` |
| contribution-pass.md | the reviewers' pass over a green head: rotation, the record | `src/runner/contributions/pass.rs`, `crates/process/src/worktree/` |
| contribution-talk.md | what contributors say: hearing, answering, the quiet clock | `src/runner/contributions/{heard,clock}.rs`, `src/runner/thread_replies.rs` |
| contribution-release.md | asking a contributor for a release: yes, thumbs up or silence, the ruling | `src/runner/contributions/{release,ruling}.rs` |
| contribution-merge.md | the merge ruling every contribution gets: yes, thanks and no | `src/runner/contributions/{merge,ruling}.rs`, `core/settings/rulings.rs` |
| representative.md | the representative: its agent file, `agents.representative`, the call that words a post | `src/runner/representative.rs`, `core/agents/front.rs`, `core/settings/agents.rs`, `agents/representative.md` |
| dependencies.md | the bumps bots open: `dependencies` settings, rebase asks, proof, merge, red | `core/settings/dependencies.rs`, `src/runner/dependencies.rs`, `src/runner/flight/proving.rs`, `src/runner/dev_server/bump.rs`, `core/state/bumps.rs`, `src/adapters/e2e_runner.rs`, `src/adapters/pages.rs`, `src/adapters/bridge/client.rs` |
| triage.md | settings, the triager, the pick rule, `since`, the verb, `status` | `core/settings/triage.rs`, `src/runner/triage.rs`, `src/runner/open_checks.rs`, `core/ports/issue.rs` |
| triage-pass.md | one issue's pass: post, labels, duplicate close, agent-ready | `src/runner/triage/`, `src/runner/flight/`, `core/ports/voice.rs` |
| triage-author.md | the author's round: the ask, `github`, `local`, `none`, answers | `src/runner/triage/{ask,heard,local}.rs`, `src/runner/representative/reading.rs` |
| triage-spam.md | the spam check: `triage.spam`, `possible-spam` | `src/runner/triage/spam.rs` |
| log-lines.md | what the runner and the dog log between steps: pause, handover, lease waits, skips, warnings | `src/runner/drain.rs`, `src/runner/skip_log.rs`, `src/runner/review_bot/lease_wait.rs`, `src/runner/claim.rs`, `dog/desk.rs`, `dog/door.rs` |
| metrics.md | kelpie's series on shep's metrics endpoint, alerts, dashboard | `core/metrics.rs`, `core/metrics/`, `core/ports/metrics.rs`, `core/metrics/shepherd_sink.rs`, `assets/prometheus/`, `assets/grafana/` |
| metrics-work-items.md | work items' series, the `finished` line | `core/usage/finished.rs`, `src/runner/ci_failure.rs` |
| metrics-dog.md | the dog's series: leases, summons, the forge hold | `core/metrics/dog.rs`, `dog/watch.rs` |
| metrics-transcripts.md | series and reports read from a call's transcript: MCP servers, tool calls, `shep kelpie usage` | `crates/facts/`, `src/runner/transcript_count.rs`, `core/usage/` |
| budget.md | pacing, dog + leases, time, usage, settings | `src/runner/pace.rs`, `dog/`, `core/usage.rs`, `core/settings/`, `src/sheep.rs` |

Each file stays under 8 KB, and an area past it splits in two with a new row here (`scripts/design-size.sh`). A changed decision updates its bullet in the same PR.

Elsewhere: `docs/design-log.md` (record, history, old file formats), `docs/facts/`, `docs/adr/`, `GLOSSARY.md`.
