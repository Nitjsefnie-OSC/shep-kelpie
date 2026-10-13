# Contribution talk

What a contributor says on their pull request, as kelpie records it. Settings, detection and the security review are in `contributions.md`. #625, #626

## Hearing

- Kelpie records what a contributor says and never replies yet. `ProjectState.contributions[].heard` is the contributor's, `context` the maintainer's, each the last 50 `Said`: `{ kind, id, at }`, kind `comment`, `reply` or `push` (a push carries `head` in place of `id`). No text is kept, a prompt reads it again, fenced by `src/runner/untrusted.rs`. Both left out while empty. State version 48. `src/state/contributions.rs`
- The author decides, by login ignoring case. The contribution's author is heard, the maintainer's go to `context`, and a bot, a third party and a comment kelpie posted through `gh` (`posted` ids) are dropped. A review reply carries no posted id, so one by the login `gh` acts as is dropped too, the maintainer's own included when that is their login. A comment and a review reply are numbered apart, so an id counts with its kind.
- One `comments_since` read per `BOARD_POLL` serves rulings' replies and contributions: `thread_job` reads when a ruling waits or a contribution is followed. Review threads come from `Forge::thread_replies_since`, one GraphQL read per followed pull request, first 100 threads and 100 comments each, through the login. `src/runner/thread_replies.rs`, `src/runner/contributions/heard.rs`
- The read reaches back `SLACK` behind the last, and what is on record is not heard again. Nor is a comment written before all a full list still holds, since it was kept once and has dropped off; a push is never refused. A review thread is read from the contribution's `reviews_from`, saved in the state file, and moved to `SLACK` behind a read that worked. A failed one is told once and read again from where it began, restart or not, apart from the repo's comments.
- A push is heard by `follow_contributions` as the head moving, stamped when the board saw it. No reaction is read (#629).
- When the maintainer's or `gh`'s login cannot be had, nothing is recorded and the place the comments are read to is held, so the next read hears all of it.

## The quiet clock

- `contributions[].waiting_since` is when kelpie began to wait on the contributor for the current head, left out while it waits on no one. State version 53. `src/runner/contributions/clock.rs`, `src/state/contributions.rs`
- `follow_clock` runs at the end of `follow_contributions`, once a poll, with `waits_on_contributor`: CI red on the head, or its pass has findings or a bot's open threads (nits too). It starts the clock at the poll's time from the runner's `Clock` port, keeps a running one's start, and clears it when the head waits on no one: green CI, no findings and no open bot thread leaves the maintainer's move.
- Reset: a contributor's comment or review reply newly heard restarts a running clock at now. A push is a new head, which clears it, and the head's own results start it again. A reaction is not read, and the maintainer's, a bot's and a stranger's words are not the contributor's, so none moves it. A clock that is not running is not started by a comment.
- `Clock::due(now)` is nothing, `Nudge` once `nudge_days` whole days have passed since `waiting_since`, or `Quiet` once `quiet_days` have. Nothing acts on it yet (#629), and red CI before a release gets no comment.
- `status` shows `waiting_since` and `clock: { waiting_for, days_to_nudge, days_to_quiet, due? }` for a contribution with one running, the days whole and 0 once reached. The plain `status` adds a line.
