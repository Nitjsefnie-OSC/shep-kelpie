# Design

Kelpie: dog of the maintainer's shepherd; one runner per project takes ready issues off a board, workers build, PRs go through reviewers, CI, merge gate. Maintainer asked only via rulings.

## Always

- Gates, locks, rate windows live in code; Claude only for work and judgement. ADR 0001
- Kelpie controls each agent's context growth. `runner/turn/`
- Naming: dog = adopted `kelpie` (lease dog); project = sheep; agent calls = lambs. One kelpie for all projects (bot windows, pacer are machine/account-wide). ADR 0001 to 0004
- Binary is the dog with no args + `SHEP_DOG_NAME` and `SHEP_NAME` set; needs the `shep adopt` channel; names itself in handshake so shep never lists it `silent`. `src/sheep/mod.rs`, shep-pm/shep#656
- Needs shep 0.15.0+ in 0.15; `upgrade` refuses a build its shepherd would. shep-pm/shep#688, #319, #656, #914
- Home: `$SHEP_HOME/kelpie` (`KELPIE_HOME`), shep plugin, folder per project; dog files stay under `$SHEP_HOME`. #253
- The shepherd is reached through the `Dial` port (`crates/core/src/shepherd/connection.rs`): `Socket` is shep's control socket; tests dial `FakeShepherd`, which answers in memory. #587
- Socket path over 103 bytes: runner refuses to start (macOS caps 104). `crates/runner/src/runner/mod.rs`
- Home never moved from `~/.kelpie`: refuses to start naming each old file, exit 78 (in `stop_exit_codes`, no restart into it). #311, #345
- State file version 76 (`VERSION` in `crates/core/src/state/mod.rs`); each change one way: rollback means finishing or dropping open items first. `crates/core/src/state/removed/mod.rs`
- Business code is Rust; Tauri GUI after MVP, until then run and watched from a Claude session. #5
- Crates under `crates/` use nothing above them; the root package holds the binary and the verbs (`src/flock/`, `src/upgrade/`, `src/sheep/`, `src/doctor/`, `src/issues/`, `src/schema.rs`) and reaches every crate. ADR 0015
- Never put budget numbers in agent prompts; never let a worker merge or post as the maintainer (see fence.md, github-app.md).

## Files

| file | covers | read when you touch (`core/` is `crates/core/src/`, `dog/` is `crates/dog/src/dog/`, `adapters/` and `github/` are in `crates/adapters/src/`, `runner/` is `crates/runner/src/runner/`, here and in every area file) |
|---|---|---|
| work.md | projects, board, adoption, slots, issue writer | `core/board/`, `src/issues/mod.rs`, `runner/adopt/`, `runner/slots.rs`, `src/flock/verbs.rs`, `core/guard/issues/` |
| pm.md | the project manager, its board file | `runner/pm/`, `runner/briefing/` |
| workers.md | runner loop, turns | `runner/turn/`, `runner/flight/` |
| fence.md | sandbox, guard, hooks, reads | `core/{profile,guard,hooks}/`, `core/dep_sources.rs`, `runner/turn/dep_reads.rs` |
| browser.md | a worker's browser beyond its sandbox: bridge, filter, tools | `core/{bridge,browse}.rs`, `adapters/bridge/`, `runner/browser.rs`, `core/settings/browser.rs`, `crates/process/src/tools/` |
| browser-dev-server.md | a worker's browser dev server: launch, port, readiness | `runner/dev_server/`, `core/ports/dev_server.rs`, `adapters/dev_server/` |
| compaction.md | when a session compacts, `worker.compact_window` per harness, `COMPACT` | `runner/compact/`, `adapters/pi/compact.rs`, `adapters/codex/context.rs`, `adapters/goose/compact.rs` |
| skills.md | step skills, repo plugins, how harnesses load them | `core/skills/`, `adapters/skills.rs`, `runner/turn/` (`prepared`) |
| agents.md | agent files, harnesses, gateways | `core/agents/`, `adapters/`, `core/settings/agents/`, `crates/process/src/spawn.rs` |
| handover.md | a runner handing in-flight turns to a new image, `upgrade` | `src/sheep/handover/mod.rs`, `runner/flight/carry/`, `adapters/process/adopt/`, `src/upgrade/handover.rs` |
| waits-and-spend.md | a turn waiting for its model or host, spend by role | `runner/turn/unreachable/`, `adapters/unreachable/`, `adapters/codex/`, `core/usage/`, `core/settings/agents/` |
| review.md | review, bots, rulings | `runner/review/`, `runner/ruling/`, `core/webhook.rs` |
| review-outcomes.md | fix turn findings' outcomes, journaled | `runner/review/outcomes/` |
| local-review.md | local reviewers (`command`, `endpoint`), their lock, rounds kept after an item | `runner/review/keep.rs`, `core/settings/local_rounds.rs`, `core/settings/local.rs`, `adapters/local/` |
| merge.md | CI, catching up, merge, after it | `runner/gate/`, `runner/merge/`, `runner/close_merged.rs`, `runner/main_ci/` |
| local-tickets.md | the local ticket store, `ticket` verbs, closing on merge | `core/local_tickets/`, `src/flock/ticket.rs`, `runner/close_merged.rs` |
| github-app.md | App tokens, posting, answers, ruling panel | `runner/voice/`, `runner/panel/`, `crates/panel/`, `core/guard/gh.rs`, `crates/worker_folder/` |
| contributions.md | others' pull requests: `contributions` settings, detection | `core/settings/contributions.rs`, `core/settings/reviewers/`, `runner/contributions/`, `core/state/contributions.rs` |
| contribution-security.md | a contribution's security review, slots, budget | `runner/contributions/`, `runner/flight/security.rs`, `crates/process/src/worktree/` |
| contribution-pass.md | reviewers' pass over a green head: rotation, the record | `runner/contributions/pass.rs`, `crates/process/src/worktree/` |
| contribution-talk.md | what contributors say: hearing, answering, quiet clock | `runner/contributions/{heard,clock}.rs`, `runner/thread_replies/` |
| contribution-release.md | asking a contributor for a release: yes, thumbs up, silence, the ruling | `runner/contributions/{release,ruling}.rs` |
| contribution-merge.md | every contribution's merge ruling: yes, thanks and no | `runner/contributions/{merge,ruling}.rs`, `core/settings/rulings.rs` |
| representative.md | the representative: its agent file, `agents.representative`, the call that words a post | `runner/representative/`, `core/agents/front/`, `core/settings/agents/`, `agents/representative.md` |
| dependencies.md | the bumps bots open: `dependencies` settings, rebase asks, proof, merge, red | `core/settings/dependencies.rs`, `runner/dependencies/`, `runner/flight/proving.rs`, `runner/dev_server/bump/`, `core/state/bumps.rs`, `adapters/e2e_runner.rs`, `adapters/pages.rs`, `adapters/bridge/client/` |
| triage.md | settings, triager, pick rule, `since`, the verb, `status` | `core/settings/triage.rs`, `runner/triage/`, `runner/open_checks.rs`, `core/ports/issue.rs` |
| triage-limits.md | the four triage limits | `runner/triage/limits.rs` |
| triage-pass.md | one issue's pass: post, labels, duplicate close, agent-ready | `runner/triage/`, `runner/flight/`, `core/ports/voice.rs` |
| triage-images.md | an issue's images | `runner/triage/images.rs`, `adapters/gh/attachment.rs` |
| triage-author.md | author's round: the ask, `github`, `local`, `none`, answers | `runner/triage/{ask,heard,local}.rs`, `runner/representative/reading.rs` |
| triage-spam.md | the spam check: `triage.spam`, `possible-spam` | `runner/triage/spam.rs` |
| log-lines.md | what the runner and dog log: pause, handover, lease waits, skips, warnings | `runner/drain/`, `runner/skip_log.rs`, `runner/review_bot/lease_wait.rs`, `runner/claim.rs`, `dog/desk.rs`, `dog/door/` |
| metrics.md | kelpie's series on shep's metrics endpoint, alerts, dashboard | `core/metrics/`, `core/ports/metrics.rs`, `core/metrics/shepherd_sink.rs`, `assets/prometheus/`, `assets/grafana/` |
| metrics-work-items.md | work items' series, the `finished` line | `core/usage/finished.rs`, `runner/ci_failure.rs` |
| metrics-dog.md | the dog's series: leases, summons, the forge hold | `core/metrics/dog.rs`, `dog/watch/` |
| metrics-transcripts.md | series read from a call's transcript: MCP servers, tool calls, `shep kelpie usage` | `crates/facts/`, `runner/transcript_count/`, `core/usage/` |
| budget.md | pacing, dog + leases, time, usage, settings | `runner/pace.rs`, `dog/`, `core/usage/`, `core/settings/`, `src/sheep/mod.rs` |

Each file stays under 8 KB, and an area past it splits in two with a new row here (`scripts/design-size.sh`). A changed decision updates its bullet in the same PR.

Elsewhere: `docs/design-log.md` (record, history, old file formats), `docs/facts/`, `docs/adr/`, `GLOSSARY.md`.
