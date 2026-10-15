# Triage

Kelpie's triage of issues other people open: the settings, the agent, the rule for which issues are untouched and the first pass over one (#887, slices 1 and 2). The first pass researches an issue, comments what it found and leaves it `needs-triage`. The slices after it add the questions and the other endings.

- `[app.dogs.kelpie.triage]`, an absent table meaning `pick = "off"`, with `deny_unknown_fields`. `src/settings/triage.rs`
  - `pick`: `off` (default), `maintainer` (only the maintainer's issues) or `all`
  - `wait_days`: whole days above 0, default 7
  - `max_questions`: 1 to 10, default 10. 0 is refused by the type and above 10 by `Triage::check`, which `Settings::from_table` calls.
  - `agent`: an agent file whose role is `triager`, default `triager`. `Settings::triager` checks it.
- `reread` keeps the running `triage` and logs it as waiting for the next start, as it does `contributions`. `src/runner/reread.rs`
- Agent file `triager.md`, role `triager`, `Expert`-shaped frontmatter, shipped on Claude Code and written by `add`. It researches an issue against a clean checkout and reports in one JSON object: `type`, `priority`, `related`, `files`, `tests`, `criteria`, `open` (`question` and `recommended`) and `body`, null in this pass. The issue text is data. `agents/triager.md`
- Every harness gives the triager the reviewer's read-only reach (`Tools::Review`): Claude Code `Read,Glob,Grep` with no MCP server, pi `read,grep,find,ls`, Codex a read-only shell with `apply_patch` refused, goose a read-only shell hook in an empty fence. `harness.triager` in `docs/parity.toml`.
- `Role::Triager` in `src/agents.rs` and `crate::ports::Role`. No work item records its calls (`src/work_item/spend.rs`, `tally.rs`), and the ledger skips it for a work item. The code graph reaches it when on (`reads_code`, `src/runner/code_graph.rs`). `CallKind::Triage` gives it a `triager` total in `usage`, apart from the work items. `src/usage.rs`
- `Runner::open` checks the settings in `src/runner/open_checks.rs`, with the contribution checks. While `triage.pick` is not `off`, the App must cover the repo (else `SettingsError::Invalid` on `triage.pick`, as `contributions.pick` does), then `triage.agent` and `agents.representative` must name files with the right role. Under `off` no file is read.
- `OpenIssue` gains `author`, `author_is_bot`, `labels` and `created`, which `gh issue list` supplies. `src/ports/issue.rs`, `src/adapters/gh/issues.rs`
- `ProjectState.triage { since }` (state version 60) is the moment `pick` went past `off`, left out while unset. Only an issue created after it is triaged, so turning triage on never reaches back over the backlog. It is set at `Runner::open` when `pick` is not `off` and none is saved, and cleared there when `pick` is `off`. No other path sets or clears it. `src/runner/triage.rs` (`settle_since`), `src/state/triage.rs`
- The pick rule runs once a `BOARD_POLL` while `pick` is not `off`, over `open_issues`. An issue is untouched when it is open and:
  - it carries none of `TRIAGE_LABELS`: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`
  - its author is not a bot, the App included
  - it has no record in `ProjectState.triage.issues`
  - it was created after `triage.since`
  - under `maintainer`, its author is `Runner::maintainer()`, ignoring case
- `status` lists them under `triage.due`, oldest first (number, title, author, created), left out when none, and plain `status` adds a `triage due` line. The list lives in memory and the next poll rebuilds it. `src/runner/triage.rs`, `src/runner/trigger.rs`, `src/flock/control/plain.rs`
- A listing that cannot be read, or a maintainer that cannot be named under `maintainer`, changes nothing and fails no poll.
- Unchanged: the board, `ready_issues`, the issue writer and its guard.

## The first pass

- `Runner::triage_due` runs from `dispatch` after `contribution_due` and before the board's pick, so it needs the board open and a free slot, and waits otherwise. It takes the issue the verb queued, then the oldest in `triage.due`, one at a time, and starts nothing while draining or where no App covers the repo. Each issue is tried once a poll. `src/runner/triage.rs`
- Before an issue's first call its comments are read once (`Forge::issue_comments`). A comment by the App's login holding `<!-- kelpie:triage -->` records the issue with no call. The marker in anyone else's comment counts for nothing. The verb's issue skips the read.
- The call is a flight: `src/runner/flight/triage.rs`, one at a time on its own thread, `CEILING` 15 minutes, `News::Triaged`. `slot_issues` counts it and a drain lists it in `calls` as role `triager`. It runs `triage.agent` with `Tools::Review` in a `worktree::view` of `origin/main` at `<project>/triage/<n>/`, removed after, with no repo plugins.
- The prompt holds the issue and the open issues' numbers and titles, both `fenced`. Code keeps a related number only if the listing holds it, a file only if the view holds it, cuts `open` at `max_questions`, and turns a `P0` into an open question. `src/runner/triage/research.rs`
- The representative words the facts (`Runner::represent_issue`), code appends the marker, `Voice::post` posts it. Then the type and priority the repo has (`Forge::repo_labels`), then `needs-triage`, made if missing. `src/runner/triage/comment.rs`
- `triage.issues` holds `{issue, comment, step}`, `step` being `commented`, `labelled` or `done`. The post is the one write with no save before it: a post whose record did not save is caught by the marker, which is read again after every post. Each later write follows a save of its step. The last 200 `done` records are kept (`TRIAGED_KEPT`). A poll finishes a record short of `done`.
- A failure is told once in the log and tried at the next poll. Five give the issue up until a restart. A busy forge does not count. A research that read but could not be posted is kept in memory and posted again at the next poll, not researched again.
- The ledger line is role `triager`, kind `triage`, with the issue.
- `shep kelpie triage <n>` queues `n` ahead of `triage.due`, past `since` and the author test. Refused when `pick` is `off`, when `n` is not open or carries a triage label, or when its call is in flight. A recorded issue is triaged again and its record is replaced once the new comment posts.
- `status` lists the call in flight under `triage.running` and each record under `triage.issues`.
