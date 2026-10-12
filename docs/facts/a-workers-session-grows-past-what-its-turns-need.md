# A worker's session grows past what its turns need

From `usage.jsonl` on four shep-kelpie work items (2026-10-10, #426), in units. Every turn after the build resumes the worker's one session and rereads all of it, and nothing compacted it:

| Issue | Build turn | Later worker turns | Context at the end |
|---|---|---|---|
| #397 | 4.49M | 5.75M (7 turns) | 486k |
| #398 | 3.21M | 7.28M (6 turns, retro included) | 399k (475k peak) |
| #361 | 3.24M | 2.55M (4 turns) | 356k |
| #367 | 2.53M | 1.41M (4 turns) | 284k |

- Cache reads were 75 to 80% of each session's units. One fix round on #398, for a ruling's note, cost 3.33M on its own, about what the whole build cost, and #398's worker read 76.8M cached tokens in six calls.
- The project manager's `COMPACT_PAST` (100k) had already found compacting as it goes the cheapest shape with nothing lost. The worker's `WORKER_COMPACT_PAST` starts at 150k; whether that is the right mark is unmeasured, and the ledger's `compact` calls are what to measure it by.
- Nothing bounded a turn itself (#476). On #465 the build turn ran 41 minutes: 279 model calls and 417 tool calls, and the session grew from 14K to 555K tokens before it ended, $12.84 on its own.
  - Claude Code compacts on its own only near the model's window, and Sonnet's is 1M, so it never did.
  - Its `autoCompactWindow` setting (or `CLAUDE_CODE_AUTO_COMPACT_WINDOW`) moves that point down, so every worker's settings file now carries `autoCompactWindow`, from `worker.compact_window`, 200000 by default, and `0` leaves it out. Reviewers and the project manager get none, and neither does kelpie's own `/compact`.
- A compaction inside a turn summarizes the turn's own prompt, and Claude Code's docs say early instructions may not survive it, among them the `thread <n>: fixed` line format kelpie reads when the turn ends.
  - So a worker's settings file also holds a `SessionStart` hook matched on `compact`, which prints the turn's prompt again from a file beside the settings file (`settings-<issue>.prompt.md`).
  - The stand-in Claude's test runs that hook from the settings file and answers with thread lines only if its output carried the format; a real compacted turn is not yet measured.
