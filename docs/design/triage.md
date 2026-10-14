# Triage

Kelpie's triage of issues other people open: the settings, the agent and the rule for which issues are untouched. Slice 1 of #887. Nothing posts or labels yet, and the slices after it add the research call, the questions and the records.

- `[app.dogs.kelpie.triage]`, an absent table meaning `pick = "off"`, with `deny_unknown_fields`. `src/settings/triage.rs`
  - `pick`: `off` (default), `maintainer` (only the maintainer's issues) or `all`
  - `wait_days`: whole days above 0, default 7
  - `max_questions`: 1 to 10, default 10. 0 is refused by the type and above 10 by `Triage::check`, which `Settings::from_table` calls.
  - `agent`: an agent file whose role is `triager`, default `triager`. `Settings::triager` checks it.
- `reread` keeps the running `triage` and logs it as waiting for the next start, as it does `contributions`. `src/runner/reread.rs`
- Agent file `triager.md`, role `triager`, `Expert`-shaped frontmatter, shipped on Claude Code and written by `add`. It researches an issue against a clean checkout and reports in one JSON object (`kind`, `priority`, `related`, `files`, `tests`, `sure`, `body`, `criteria`, `questions`) that slice 2 parses. The issue text is data. Its `body` holds the issue writer's shape: `Touches:`, `## What to build`, `## Acceptance criteria` with named tests. `agents/triager.md`
- Every harness gives the triager the reviewer's read-only reach (`Tools::Review`): Claude Code `Read,Glob,Grep` with no MCP server, pi `read,grep,find,ls`, Codex a read-only shell with `apply_patch` refused, goose a read-only shell hook in an empty fence. `harness.triager` in `docs/parity.toml`.
- `Role::Triager` in `src/agents.rs` and `crate::ports::Role`. No work item records its calls (`src/work_item/spend.rs`, `tally.rs`), no code graph reaches it, and the ledger skips it for a work item. `CallKind::Triage` gives it a `triager` total in `usage`, apart from the work items. `src/usage.rs`
- `Runner::open` checks the settings in `src/runner/open_checks.rs`, with the contribution checks. While `triage.pick` is not `off`, the App must cover the repo (else `SettingsError::Invalid` on `triage.pick`, as `contributions.pick` does), then `triage.agent` and `agents.representative` must name files with the right role. Under `off` no file is read.
- `OpenIssue` gains `author`, `author_is_bot`, `labels` and `created`, which `gh issue list` supplies. `src/ports/issue.rs`, `src/adapters/gh/issues.rs`
- `ProjectState.triage { since }` (state version 59) is the moment `pick` went past `off`, left out while unset. Only an issue created after it is triaged, so turning triage on never reaches back over the backlog. It is set at `Runner::open` when `pick` is not `off` and none is saved, and cleared there when `pick` is `off`. No other path sets or clears it. `src/runner/triage.rs` (`settle_since`), `src/state/triage.rs`
- The pick rule runs once a `BOARD_POLL` while `pick` is not `off`, over `open_issues`. An issue is untouched when it is open and:
  - it carries none of `TRIAGE_LABELS`: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`
  - its author is not a bot, the App included
  - it has no record in `ProjectState.triage` (slice 2 adds them; none exists yet)
  - it was created after `triage.since`
  - under `maintainer`, its author is `Runner::maintainer()`, ignoring case
- `status` lists them under `triage.due`, oldest first (number, title, author, created), left out when none, and plain `status` adds a `triage due` line. The list lives in memory and the next poll rebuilds it. `src/runner/triage.rs`, `src/runner/trigger.rs`, `src/flock/control/plain.rs`
- A listing that cannot be read, or a maintainer that cannot be named under `maintainer`, changes nothing and fails no poll.
- Unchanged: the board, `ready_issues`, the issue writer and its guard.
