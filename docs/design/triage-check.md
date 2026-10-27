# The issue check on every issue

ADR 0018, cut 3: the board runs the issue check (`work.md`) on every new or edited open issue, and the triager runs only on one that fails. The settings and pick rule are in `triage.md`, and one issue's pass is in `triage-pass.md`.

- Each triage poll lists the open issues through `ports.tickets`, whatever `triage.pick` says, `off` included. `check_issues` then checks each issue a bot did not open that is new to `checks`, whose key changed, or whose triage ended since. An issue under a pass waits for a listing made after the pass ends (`triage_busy`: an unfinished record, a reply or comment not yet posted, or the call in flight). Under `off`, a listing that fails waits for the next poll, so the check asks the forge at most once a poll. `runner/triage/{checks,poll_read}.rs`
- `ProjectState.checks` (state version 89) holds one `{issue, key, failures, triage}` per listed issue. An issue that leaves the listing is dropped.
  - `key` is `board::check::key`, an FNV-1a hash (`check::fnv1a`, the one the `local` wait uses) over the trimmed title, the trimmed body, and the priority and `kind:` labels, lowered and sorted. No other label goes in, so kelpie's own `TRIAGE_LABELS`, `in-progress` and `agent:` labels never set off a new check.
  - `failures` holds each `check::Failure` in the check's words. It is empty for a pass.
  - `triage` is `owed` (it failed and no triager has run since), `edited` (it failed on an edit made after its triage ended), `ran` (the triager ran on this edit, and its end is checked again), or left out.
  - A write that fails keeps the old result, so the next poll tries again. A save that fails keeps the results in memory and leaves a note. `crates/core/src/state/checks.rs`
- A pass needs no triager call. The maintainer's issue (`Runner::maintainer()`, ignoring case) gets `ready-for-agent`, made if the repo lacks it, and loses `needs-triage`. Anyone else's gets `needs-triage` and waits for the maintainer to label it by hand, as `hand_ready` says: kelpie never labels it ready. An issue with any other triage label, or with a work item, is left as it is.
- A failure is owed the triager, which the pick rule reaches as before: none under `off`, the maintainer's under `maintainer`, everyone's under `all`, and only after `since` and above `pruned_through`. An issue with a triage record and no earlier result was triaged before this check existed, and is owed nothing until it is edited. An edit made after its triage ended is `edited`, which `first_pass_owed` reads as owing a new first pass on a `done` or `ready` record.
- A `ready-for-agent` issue that failed is held off the board, whatever `hand_ready` says. `check_holds` gives it the `check` skip (`board::Skip::Check`, shown under `skipped` in `status` with `reason: check` and the failures) while its result is for the text the board read or its triage is `ran`. A finished or dropped ticket is not held. For the due list, `ready-for-agent` is no decision unless the issue is being worked on: a `taken` record, a work item, an open pull request that closes it, or pull requests that were not read. `runner/dispatch/mod.rs`, `runner/briefing/`
- The triager's end is checked again: a result at `ran` is checked on the next listing, whatever its key.
  - A pass needs nothing more.
  - A failure whose key changed, on the maintainer's issue with no triage label put on by hand, gets one comment. It names each failure and says the issue waits at `needs-triage` until it is edited. The result is then plain, and the issue is not triaged again until its key changes.
  - A failure whose key did not change (a pass that wrote nothing the check reads, as silence leaves it) gets no comment.
  - Another person's issue gets no such comment, since the check's words are not plain and its triage comment already handed it to the maintainer.

## Today's triage steps

- `ready` stays. The check's pass now reaches it, and the `sure` rule is gone (`triage-pass.md`).
- `needs-triage` stays as the queue label. A rewrite that still fails goes under the maintainer's text and ends there, and so does another person's passing issue.
- `triage.pick`, `triage.since` and `pruned_through` stay, but they decide only which failing issues reach the triager. The check runs whatever they say.
- `hand_ready` keeps its meaning for another person's passing issue, which waits, and for the take-back swap. The maintainer's failing hand-labelled issue is held and triaged under either value (`triage-hand.md`).
- The author's round, `wait_days` and `local` stay, now only for questions the triager has no good answer for.
