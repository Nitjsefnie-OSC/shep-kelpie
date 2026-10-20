# A ticket scoped too big

What a work item costs when its issue reads small and its change is not. Measured 2026-10-19 on #1010 and its pull request #1038, from the steps log, the runner log, the worker's pi session and the issues' rulings.

## #1010: the state file keys work by ticket id

The last step of ADR 0014's move from issue numbers to `TicketId`. The issue was 849 bytes: three bullets under "What it should do" and three acceptance criteria (a version 71 migration test, a `k12` round trip, tests and clippy). It opened with "With every caller on `TicketId`", which #1028, #1029 and #1031 had made true for callers but not for the stored fields every caller reads.

Changing a stored field's type reaches every reader of it. At the base, 119 source files named one of the fields the issue changed, and 30 called the `ticket()` it removed. The issue named none of them.

| | files changed | lines added / removed |
|---|---|---|
| kelpie's merged pull requests, median (260) | 15 | |
| the same, 90th percentile | 36 | |
| #1028, turns and reviews | 36 | +625 / -361 |
| #1031, board and dispatch | 75 | +1,306 / -870 |
| #1029, reports, usage and retros | 118 | +1,410 / -853 |
| #1038, the state file, at merge | 242 | +4,235 / -3,231 |

#1038 changed twice as many files as the largest kelpie pull request before it, and 16 times the median.

## What the worker did

One pi session on iq3-xxs, 12:16 to 16:51 UTC, about 4.5 hours over six turns:

- four turns ran to the one-hour ceiling with nothing committed. Each raised a ruling, and each woke the advisor, the researcher and the project manager
- three turns between them failed within seconds on `cannot empty .../settings-1010.tmp: directory not empty` (#1035), each another ruling
- the panel answered yes every time, on the grounds that two hours of uncommitted work would be stranded by a no. The advisor noted the branch still sat at its base with an empty diff, and that the design log calls a second ceiling likely stuck
- the project manager held #1011, #1012 and #1013 behind it the whole time
- the session: 1,015 shell calls (450 `grep`, 209 `sed`, 58 `cargo check`, 6 `cargo test`), 226 edits across 120 files, 46 tool errors, 6 compactions, about 1.02M input tokens

The work landed as one commit, `refactor(state)!`, plus a docs commit and a clippy commit. The review and CI rounds added eight more.

## What the worker added past the issue

The pull request's own "Decisions I made" list goes past the three bullets: `shep kelpie rule` taking ticket ids, `status` and `board` printing them, the in-flight ledger's sentinel keys turned into a `Key` enum, the handover's `Carry::FORMAT` 5 to 6, and five files split past 1,000 lines. Some of it the type change forced (a ruling's id is a `TicketId` now). Some of it was a choice the issue left open. It also deferred four gaps to the follow-ups file.

## Review

The pull request opened at 16:51 UTC and merged at 18:59, about two hours later:

- CI failed twice: `docs` and `test` on the first head, `docs` again on the second (the design area went over its size cap)
- one more turn hit the ceiling, after a compaction failed with `generation hit the token cap and the summary is incomplete`. Another ruling, and another advisor and researcher call
- two review rounds: the Codex bot found one high and one medium, then defect-hunter-lite found one low. Each took one fix turn
- all three findings sit in local-ticket handling the issue did not ask for: two in `src/flock/rule.rs` (`rule` taking ticket ids, from the worker's own decisions list) and one in `crates/runner/src/runner/browser.rs`

Three findings on a 7,466-line diff is little review for its size. Whether that means the change was sound or the reviewers could not hold it is for the escape count to say, once fixes have had a week to land.

## What it says

- A refactor that changes a stored type is sized by its readers, not by its bullets. A count of the files that name the field (`git grep -l`) at issue time would have said 119.
- Splitting by module, as #1028, #1029 and #1031 did for callers, kept those at 36 to 118 files. The stored fields could have gone the same way: the migration and new version first, with a shim the readers keep using, then the readers by module.
- What the worker adds past the issue is where review finds the bugs. All three of #1038's findings were in the ticket-id handling of `rule`, which the issue never asked for.
- A ceiling hit with nothing committed is the tell. Four of them on one item, each answered yes, cost four hours without a commit, and with the three failures, seven rulings and fourteen advisor and researcher calls.
