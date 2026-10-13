# shep-kelpie

Kelpie is a shep dog that runs Claude Code workers from a planned work item to a merged pull request, holding gates, leases and pacing in code. MIT OR Apache-2.0.

## Where things stand

The design is settled and the pillars are measured. The first build (#5) is done, and kelpie now runs its maintainer's projects, this one included, taking one issue from the board at a time per worker. The code is one Rust crate, `shep-kelpie`, at the repo root, with `#![forbid(unsafe_code)]`.

- `docs/design/`: every current decision, its reason and where it lives. Read its README before writing code, then the area files its table names for the code you touch.
- `docs/design-log.md`: the full record behind those decisions, their mechanics and history, and the test plan. Search it for the detail on one decision.
- `docs/facts/`: the measured facts, one file per topic.
- `docs/specs/`: one spec per test series, and its results. The experiment code and raw results live in shep-pm/kelpie-lab, and results here cite its commits as "the experiments repo".
- `docs/adr/`: decisions that are hard to reverse. A change that contradicts one names the ADR and argues for reopening it. Its README lists every option decided against, so check it before proposing one.

## Vocabulary

`GLOSSARY.md` is the vocabulary. Use its terms exactly, in prose, code and issue titles: worker, crew, work item, phase, reset, handoff, lease, summon, ruling. In shep's terms, kelpie is a dog, a project is a sheep, and a worker is one of its lambs.

## Kelpie and shep

Kelpie builds on the published shep crates (`shep-client`, `shep-core`, `shep-channel`) and ships on its own release cycle. When kelpie needs something from shep that any dog could use, file it on shep-pm/shep as a general feature, the way shep-pm/shep#623 and #624 were.

Gotchas:

- Keep a shepherd's `SHEP_HOME` short. Its control socket is `$SHEP_HOME/run/shep.sock`, and macOS caps a socket path at 104 bytes. Kelpie uses `~/.kelpie/shep`.
- `shep trigger <sheep> <action> [params]` takes params positionally.
- Headless Claude Code has its own traps (stdin, cumulative cost, cache sharing). They are in `docs/facts/headless-claude-code.md`.

## Code style

Invoke the `rust-house-style` skill before writing or reviewing Rust. The rules are shep-pm/rust-house-style, IR-1 to IR-48.

Read `CODING_STANDARDS.md` when reviewing a change.

CI (`.github/workflows/test.yml`) runs `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, rustdoc with `-D warnings`, and `cargo +1.88 check` for the MSRV. Before handing a branch over, run fmt, clippy and the tests for what you changed (`cargo test --lib <module path>`, or `--test <name>` for a file in `tests/`). Leave the whole suite, rustdoc and the MSRV check to CI, which runs them on every push. The full suite takes about six minutes, and the MSRV check builds a second copy of every dependency. `.github/workflows/file-size.yml` also fails any `.rs` file over 1000 lines (IR-48). Run `scripts/file-size.sh` before you push, and again after merging `main`: it is the same check.

## Working an issue

Kelpie works this repo too, so these hold for a kelpie worker as for anyone:

- Read the issue, its parent spec #5, `GLOSSARY.md`, `docs/adr/` and `docs/design/` (its README, then the area files for what you touch) first. The design is settled there; ask only for a decision they leave open.
- Tests follow #5's testing decisions: the runner's work-item loop through its stand-ins, the dog's lease book with a fake clock, and adapter parsing against real recorded output. Tests assert only what the outside world sees.
- One cargo command shape for the session, and one cargo command at a time.
- A worker's sandbox binds no socket or port, so some tests fail there and pass in CI. `docs/agents/sandbox-red-tests.md` lists them with the reasons. Run what can run, and say in the pull request which tests only CI ran.
- Kelpie runs workers on three harnesses: Claude Code, pi and Codex. A change to one adapter (`src/adapters/claude*`, `pi*`, `codex*`) or to code that branches on `AgentHarness` reaches pi in the same pull request, or the pull request names the open pi issue that does. Codex may follow pi in its own issue. `CODING_STANDARDS.md` has the rule.
- New measured facts go in their topic's file under `docs/facts/`, or a new one listed in its README, in the same pull request. A changed decision updates its file in `docs/design/` too.
- Done when every acceptance criterion in the issue is met, with the evidence in the pull request's body, which ends `Resolves #<n>`.

## Writing and git

- Committed text calls the maintainer "the maintainer" and uses repo-relative paths, because the repo is public.
- Commit subjects are conventional: `type(scope): summary`, with types `feat` `fix` `perf` `refactor` `docs` `test` `ci` `chore` `style`, and `!` on the commit that breaks something. Bodies carry the full reasoning. `.githooks/commit-msg` checks locally (run `git config core.hooksPath .githooks` once per clone) and `.github/workflows/commits.yml` checks every pull request.
- Pull request titles are conventional too. `merge_commit_title` is set to the PR title, so a merge commit's subject is the title.
- Merge with a merge commit, never a squash. Release pull requests are the exception, and their workflow squashes them itself.
- One commit per item. Work reaches `main` through pull requests.
- release-plz writes `CHANGELOG.md` from commit subjects, so never hand-write an entry. Its workflows stay off until the maintainer sets `RELEASES` to `on`.

## Agent skills

### Issue tracker

GitHub Issues on `shep-pm/shep-kelpie`, via `gh`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default roles, label string equal to role name. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` at the root, ADRs in `docs/adr/`, `docs/design/`, `docs/design-log.md` and `docs/facts/`. See `docs/agents/domain.md`.
