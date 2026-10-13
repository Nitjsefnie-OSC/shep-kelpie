# Design

Kelpie: dog of the maintainer's shep shepherd; one runner per project takes ready issues off a board, agent workers build, PRs go through reviewers, CI, merge gate. Maintainer asked only via rulings.
Rules live in Rust code; a model is called only for work and judgement.

## Always

- Gates, locks, rate windows live in code; Claude only for work and judgement. ADR 0001
- Kelpie controls each agent's context growth. `src/runner/turn.rs`
- Naming: dog = adopted `kelpie` (lease dog); project = sheep (`runner <project>`); agent calls = lambs. One kelpie for all projects (GPU lock, bot windows, pacer are machine/account-wide). ADR 0001 to 0004
- Binary is the dog only with no args + `SHEP_DOG_NAME` and `SHEP_NAME` set; needs shepherd channel from `shep adopt`; names itself in handshake so shep never lists it `silent`. `src/sheep.rs`, shep-pm/shep#656
- Needs shep 0.12.4+ in 0.12 (first whose stop sweeps a sheep's lamb tree); `upgrade` refuses a build its shepherd would refuse. shep-pm/shep#688, #319
- Home: `$SHEP_HOME/kelpie` (`KELPIE_HOME` overrides), shep plugin, folder per project (no project may take a shared name); dog files stay under `$SHEP_HOME`. #253
- The shepherd is reached through the `Dial` port (`src/shepherd/connection.rs`): `Socket` is shep's control socket, and tests dial `FakeShepherd`, which answers in memory and binds nothing. A test binds a real socket only to prove the socket itself; the sandbox-red list names those. #587
- Socket path over 103 bytes: runner refuses to start (macOS caps 104). `src/runner.rs`
- Home never moved from `~/.kelpie`: refuses to start naming each old file, exit 78 (in `stop_exit_codes`, no restart into refusal). #311, #345
- State file version 37 (`VERSION` in `src/state.rs`); each change one way, so rollback means finishing or dropping open items first. `src/state/removed.rs`
- Rust for all business code; Tauri GUI after MVP, until then run and watched from a Claude session. #5
- Never put budget numbers in agent prompts; never let a worker merge or post as the maintainer (see workers.md, github-app.md).

## Files

| file | covers | read when you touch |
|---|---|---|
| work.md | projects, board, slots, issue writer, PM | `src/board.rs`, `src/issues.rs`, `src/runner/slots.rs`, `src/runner/pm.rs`, `src/flock/verbs.rs`, `src/guard/issues.rs` |
| workers.md | runner loop, turns, fence, skills | `src/runner/turn.rs`, `src/profile.rs`, `src/guard.rs`, `src/skills.rs`, `src/hooks.rs` |
| agents.md | agent files, harnesses, gateways | `src/agents/`, `src/adapters/`, `src/settings/agents.rs`, `src/spawn.rs` |
| review.md | review, bots, rulings | `src/runner/review/`, `src/runner/ruling.rs`, `src/webhook.rs` |
| merge.md | CI, catching up, merge, after the merge | `src/runner/gate.rs`, `src/runner/merge.rs`, `src/runner/close_merged.rs` |
| github-app.md | App tokens, posting, answers, ruling panel | `src/runner/voice/`, `src/runner/panel/`, `src/panel/`, `src/guard/gh.rs`, `src/worker_folder.rs` |
| contributions.md | other people's pull requests: `contributions` settings | `src/settings/contributions.rs`, `src/settings/reviewers.rs` |
| budget.md | pacing, dog + leases, time, usage, settings | `src/runner/pace.rs`, `src/dog/`, `src/usage.rs`, `src/settings/`, `src/sheep.rs` |

Each file stays under 8 KB, and a changed decision updates its bullet in the same PR. An area past 8 KB splits in two, with a new row here. `scripts/design-size.sh` checks both, in CI too.

## Elsewhere

- `docs/design-log.md`: full record, mechanics, history, old file formats
- `docs/facts/`: measurements
- `docs/adr/`: hard-to-reverse decisions
- `GLOSSARY.md`: vocabulary
