# Design

Kelpie: dog of the maintainer's shepherd; one runner per project takes ready issues off a board, workers build, PRs pass reviewers, CI, merge gate. Maintainer asked only via rulings.

## Always

- Gates, locks, rate windows live in code; Claude only for work and judgement. ADR 0001
- Kelpie controls each agent's context growth. `runner/turn/`
- Naming: dog = adopted `kelpie` (lease dog); project = sheep; agent calls = lambs. One kelpie for all projects (bot windows, pacer are machine/account-wide). ADR 0001 to 0004
- Binary is the dog with no args + `SHEP_DOG_NAME` and `SHEP_NAME` set; needs the `shep adopt` channel; names itself in the handshake, so never `silent`. `src/sheep/mod.rs`, shep-pm/shep#656
- Needs shep 0.15.0+ (ADR 0016); `upgrade` refuses builds it would. shep-pm/shep#688, #319, #656, #914
- Home: `$SHEP_HOME/kelpie` (`KELPIE_HOME`), shep plugin, folder per project; dog files stay in `$SHEP_HOME`. #253
- The shepherd is reached through the `Dial` port (`core/shepherd/connection.rs`): `Socket` is the real one; tests dial `FakeShepherd`. #587
- Socket path over 103 bytes: runner refuses to start (Windows: pipes, `core/ipc`). #1147
- Home never moved from `~/.kelpie`: refuses to start naming each old file, exit 78 (`stop_exit_codes`). #311, #345
- State file version 82 (`VERSION` in `core/state/mod.rs`); each change one way: rollback = finish or drop open items. `core/state/removed/mod.rs`
- Business code is Rust; Tauri GUI after MVP, a Claude session watches till then. #5
- Crates under `crates/` use nothing above them; the root holds the binary and verbs (`src/{flock,upgrade,sheep,doctor,issues}/`, `src/schema.rs`) and reaches every crate. ADR 0015
- No budget numbers in agent prompts; no worker merges/posts as the maintainer (fence.md, github-app.md).

## Files

| file | covers | read when you touch (`core/` is `crates/core/src/`, `dog/` is `crates/dog/src/dog/`, `adapters/` is `crates/adapters/src/`, `runner/` is `crates/runner/src/runner/`, here and in every area file) |
|---|---|---|
| work.md | projects, board, adoption, slots, issue writer | `core/board/`, `src/issues/mod.rs`, `runner/adopt/`, `runner/slots.rs`, `src/flock/verbs.rs`, `core/guard/issues/` |
| pm.md | the project manager, its board | `runner/pm/`, `runner/briefing/` |
| workers.md | runner loop, turns | `runner/turn/`, `runner/flight/` |
| runner-lock.md | the runner's lock, its hold metric | `runner/{lock_hold,off_lock}.rs`, `runner/trigger/` |
| fence.md | sandbox, guard, hooks, reads | `core/{profile,guard,hooks}/`, `core/dep_sources.rs`, `runner/turn/dep_reads.rs` |
| browser.md | a worker's browser: bridge, filter, tools | `core/{bridge,browse}.rs`, `adapters/bridge/`, `runner/browser.rs`, `core/settings/browser.rs`, `crates/process/src/tools/` |
| browser-dev-server.md | a worker's dev server: launch, port, readiness | `runner/dev_server/`, `core/ports/dev_server.rs`, `adapters/dev_server/` |
| compaction.md | when a session compacts, `worker.compact_window` per harness, `COMPACT` | `runner/compact/`, `adapters/pi/compact.rs`, `adapters/codex/context.rs`, `adapters/goose/compact.rs` |
| skills.md | step skills, repo plugins, how harnesses load them | `core/skills/`, `adapters/skills.rs`, `runner/turn/` (`prepared`) |
| agent-files.md | agent file, `extends`, baselines | `core/agents/`, `src/flock/add/mod.rs` |
| agents.md | harnesses, gateways | `adapters/`, `core/settings/agents/`, `process/spawn.rs` |
| cli-adapter.md | a new CLI | `adapters/cli/` |
| handover.md | in-flight turns handed to a new image, `upgrade` | `src/sheep/handover/mod.rs`, `runner/flight/carry/`, `adapters/process/adopt/`, `src/upgrade/handover.rs` |
| windows.md | `sh`, `env`, test fakes on Windows | `process/spawn.rs`, `test_support/` |
| waits-and-spend.md | waits, held-up turns, failing steps, spend | `runner/{turn/unreachable,looks}/`, `adapters/unreachable/`, `adapters/codex/`, `core/usage/`, `core/settings/agents/` |
| review.md | review, bots, rulings | `runner/review/`, `runner/ruling/`, `core/webhook.rs` |
| review-outcomes.md | fix turn findings' outcomes | `runner/review/outcomes/` |
| local-review.md | local reviewers, lock, kept rounds | `runner/review/keep.rs`, `core/settings/local_rounds.rs`, `core/settings/local.rs`, `adapters/local/` |
| merge.md | CI, catching up, merge, after it | `runner/gate/`, `runner/merge/`, `runner/close_merged.rs`, `runner/main_ci/` |
| local-tickets.md | the local ticket store, `ticket` verbs, closing on merge | `core/local_tickets/`, `src/flock/ticket.rs`, `runner/close_merged.rs` |
| github-app.md | App tokens, posting, answers, ruling panel | `runner/voice/`, `runner/panel/`, `crates/panel/`, `core/guard/gh.rs`, `crates/worker_folder/` |
| contributions.md | others' pull requests: `contributions` settings, detection | `core/settings/contributions.rs`, `core/settings/reviewers/`, `runner/contributions/`, `core/state/contributions.rs` |
| contribution-security.md | a contribution's security review | `runner/contributions/`, `runner/flight/security.rs`, `crates/process/src/worktree/` |
| contribution-pass.md | reviewers' pass over a green head | `runner/contributions/pass.rs`, `crates/process/src/worktree/` |
| contribution-talk.md | what contributors say, answers, quiet clock | `runner/contributions/{heard,clock}.rs`, `runner/thread_replies/` |
| contribution-push.md | a fork worker's push; the red-CI comment | `runner/contributions/{push,after}.rs` |
| contribution-release.md | asking for a release: yes, thumbs up, silence, ruling | `runner/contributions/{release,ruling}.rs` |
| contribution-merge.md | a contribution's merge ruling: yes, thanks, no | `runner/contributions/{merge,ruling}.rs`, `core/settings/rulings.rs` |
| representative.md | its agent file, `agents.representative`, the call that words a post | `runner/representative/`, `core/agents/front/`, `core/settings/agents/`, `agents/representative.md` |
| dependencies.md | bot bumps: rebase, proof, merge, red | `core/settings/dependencies.rs`, `runner/dependencies/`, `runner/flight/proving.rs`, `runner/dev_server/bump/`, `core/state/bumps.rs`, `adapters/e2e_runner.rs`, `adapters/pages.rs`, `adapters/bridge/client/` |
| triage.md | settings, triager, pick rule, `since`, the verb, `status` | `core/settings/triage.rs`, `runner/triage/`, `runner/open_checks.rs`, `core/ports/issue.rs` |
| triage-limits.md | the limits | `runner/triage/limits.rs` |
| triage-pass.md | one issue's pass: post, labels, duplicate close, agent-ready | `runner/triage/`, `runner/flight/`, `core/ports/voice.rs` |
| triage-images.md | an issue's images | `runner/triage/images.rs`, `adapters/gh/attachment.rs` |
| triage-author.md | author's round: ask, answers, confirming, silence | `runner/triage/{ask,heard}.rs` |
| triage-local.md | local session: prompt, body hash | `runner/triage/local.rs` |
| triage-spam.md | the spam check: `triage.spam`, `possible-spam` | `runner/triage/spam.rs` |
| triage-hand.md | `hand_ready` take-back | `runner/triage/hand.rs` |
| log-lines.md | runner and dog log lines | `runner/drain/`, `runner/skip_log.rs`, `runner/review_bot/lease_wait.rs`, `runner/claim.rs`, `dog/desk.rs`, `dog/door/` |
| metrics.md | kelpie's series on shep's metrics endpoint, alerts, dashboard | `core/metrics/`, `core/ports/metrics.rs`, `assets/prometheus/`, `assets/grafana/` |
| metrics-work-items.md | work-item series, `finished` line | `core/usage/finished.rs`, `runner/ci_failure.rs` |
| metrics-dog.md | leases, summons, forge hold/calls/rate | `core/metrics/{dog,forge}.rs`, `dog/watch/` |
| metrics-transcripts.md | series from a call's transcript: MCP servers, tool calls, `shep kelpie usage` | `crates/facts/`, `runner/transcript_count/`, `core/usage/` |
| budget.md | pacing, dog + leases, time, usage, settings | `runner/pace.rs`, `dog/`, `core/usage/`, `core/settings/`, `src/sheep/mod.rs` |

Each file stays under 8 KB; over it, split in two with a new row here (`scripts/design-size.sh`). A changed decision updates its bullet in its PR.

Elsewhere: `docs/design-log.md` (record, history, old file formats), `docs/facts/`, `docs/adr/`, `GLOSSARY.md`.
