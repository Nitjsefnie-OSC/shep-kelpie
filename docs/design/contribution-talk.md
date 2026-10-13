# Contribution talk

What a contributor says on their pull request, as kelpie records it. Settings, detection and the security review are in `contributions.md`. #625

## Hearing

- Kelpie records what a contributor says and never replies yet. `ProjectState.contributions[].heard` is the contributor's, `context` the maintainer's, each the last 50 `Said`: `{ kind, id, at }`, kind `comment`, `reply` or `push` (a push carries `head` in place of `id`). No text is kept, a prompt reads it again, fenced by `src/runner/untrusted.rs`. Both left out while empty. State version 48. `src/state/contributions.rs`
- The author decides, by login ignoring case. The contribution's author is heard, the maintainer's go to `context`, and a bot, a third party and a comment kelpie posted through `gh` (`posted` ids) are dropped. A review reply carries no posted id, so one by the login `gh` acts as is dropped too, the maintainer's own included when that is their login. A comment and a review reply are numbered apart, so an id counts with its kind.
- One `comments_since` read per `BOARD_POLL` serves rulings' replies and contributions: `thread_job` reads when a ruling waits or a contribution is followed. Review threads come from `Forge::thread_replies_since`, one GraphQL read per followed pull request, first 100 threads and 100 comments each, through the login. `src/runner/thread_replies.rs`, `src/runner/contributions/heard.rs`
- The read reaches back `SLACK` behind the last, and what is on record is not heard again. Nor is a comment written before all a full list still holds, since it was kept once and has dropped off; a push is never refused. A review thread is read from the contribution's `reviews_from`, saved in the state file, and moved to `SLACK` behind a read that worked. A failed one is told once and read again from where it began, restart or not, apart from the repo's comments.
- A push is heard by `follow_contributions` as the head moving, stamped when the board saw it. No reaction is read (#629).
- When the maintainer's or `gh`'s login cannot be had, nothing is recorded and the place the comments are read to is held, so the next read hears all of it.
