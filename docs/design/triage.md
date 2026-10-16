# Triage

Kelpie's triage of issues other people open (#887): the settings, the agent, the pick rule, the verb and `status`. One issue's pass, its duplicate close and its agent-ready branch are in `triage-pass.md`, and the author's round, its ask and the author's own session are in `triage-author.md`.

- `[app.dogs.kelpie.triage]`, an absent table meaning `pick = "off"`, with `deny_unknown_fields`. `src/settings/triage.rs`
  - `pick`: `off` (default), `maintainer` (only the maintainer's issues) or `all`
  - `wait_days`: whole days above 0, default 7
  - `max_questions`: 1 to 10, default 10, refused above 10 by `Triage::check`.
  - `agent`: an agent file whose role is `triager`, default `triager`. `Settings::triager` checks it.
- `reread` keeps the running `triage` as waiting for the next start, as it does `contributions`. `src/runner/reread.rs`
- Agent file `triager.md`, role `triager`, `Expert`-shaped frontmatter, shipped on Claude Code and written by `add`. It researches an issue against a clean checkout and reports in one JSON object: `type`, `priority`, `related`, `addressed_by`, `files`, `tests`, `criteria`, `open` and `body`, the last a rewrite of the issue text for an agent. The issue text is data. `agents/triager.md`
- Every harness gives the triager the reviewer's read-only reach (`Tools::Review`), per `harness.triager` in `docs/parity.toml`.
- `Role::Triager` in `src/agents.rs` and `crate::ports::Role`. No work item records its calls (`src/work_item/spend.rs`, `tally.rs`). The code graph reaches it when on (`reads_code`, `src/runner/code_graph.rs`), with its own index (`ProjectPaths::triage_graph`), removed when the call ends. `CallKind::Triage` gives it a `triager` total in `usage`, apart from the work items. `src/usage.rs`
- `Runner::open` checks the settings in `src/runner/open_checks.rs`. While `triage.pick` is not `off`, the App must cover the repo (as `contributions.pick` does), then `triage.agent` and `agents.representative` must name files with the right role. Under `off` no file is read.
- `OpenIssue` carries `author`, `author_is_bot`, `labels` and `created` from `gh issue list`. `src/ports/issue.rs`, `src/adapters/gh/issues.rs`
- `ProjectState.triage { since }` (state version 60) is the moment `pick` went past `off`, left out while unset. Only an issue created after it is triaged, so turning triage on never reaches back over the backlog. Only `Runner::open` sets it, when `pick` is not `off` and none is saved, and clears it under `off`. `src/runner/triage.rs` (`settle_since`), `src/state/triage.rs`
- The pick rule runs once a `BOARD_POLL` while `pick` is not `off`, over `open_issues`. An issue is untouched when it is open and carries none of `TRIAGE_LABELS`, no bot opened it (the App included), it has no record in `ProjectState.triage.issues`, it was created after `triage.since`, and under `maintainer` `Runner::maintainer()` opened it, ignoring case.
- `status` lists them under `triage.due`, oldest first (number, title, author, created), left out when none; plain `status` adds a `triage due` line. The list lives in memory, rebuilt by the next poll. `src/runner/triage.rs`, `src/runner/trigger.rs`, `src/flock/control/plain.rs`
- A listing that cannot be read, or a maintainer that cannot be named, fails no poll.
- `shep kelpie triage <n>` queues `n` ahead of `triage.due`, past `since` and the author test. Refused when `pick` is `off`, when `n` is not open or carries a triage label, or when its call is in flight. A recorded issue is triaged again, its record replaced once the new comment posts.
- `status` lists the call in flight under `triage.running` and the records under `triage.issues`, a closed one with its `duplicate_of`.
