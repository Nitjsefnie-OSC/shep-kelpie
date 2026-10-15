# Contribution release

How kelpie asks a contributor for a release and what it takes as one. ADR 0010. Talk and the quiet clock are in `contribution-talk.md`. #629

## The ask

- `follow_release` runs once a poll, after `follow_posts` and before `follow_answers`, for a contribution with a running clock. At `Clock::due` of `Nudge` the representative posts one comment through `Voice::post`: it names the review bots' open threads (only when a bot answered the pass; findings and nits as `flagged_lines`) and asks whether kelpie may take the pull request from here, by comment or by a thumbs up or down on it. `src/runner/contributions/release.rs`
- `contributions[].asked` is `{ comment, at, asks, nos?, read?, closing?, thanked? }`, left out while none is open. `comment` is the ask's id, `asks` and `nos` count asks and nos, `read` is the last 50 `Said` replies read as `other`, `closing` is the maintainer's close, `thanked` that its thanks was posted. A push or a cleared clock drops it, unless it is closing.
- An ask first made at `Quiet` (a contributor who was never nudged) restarts the clock, so they have the whole window. One made at `Nudge` keeps it.

## The answer

- A reply the contributor wrote after the ask, not in `answered` or `asked.read`, is read oldest first by one tool-less representative call, its text fenced as data: `{ "reading": "yes" | "no" | "other" }`. The verdict is kept in memory under `read:{n}:{key}` so a refused post is not read twice. `other` is a conversation: `answer.rs` replies to it as to any comment.
- A reaction is read with `Forge::comment_marks` on the ask's id only, the contributor's login only. A thumbs up is a yes, a thumbs down a no, and a down wins over an up. A reaction on any other comment is never read.
- While an ask is open, a reply to it that the release stage has not read is left out of `follow_answers`, so a yes or no is never answered in public if its reading or its line failed. A reply whose reading failed five times is treated as `other`. A reaction or reply that cannot be read stops the poll's silence release, so a release never rests on a no that was not read. The next poll reads again.

## A release

- A yes, a thumbs up, or an ask open at `Quiet` with no answer is a release: `contributions[].released` is `{ at, how }`, `how` one of `yes`, `thumbs-up`, `silence` or `maintainer`. The representative posts one line that the pull request is kelpie's now, then the release is recorded. After five failed posts it is recorded without the line. Nothing merges or touches the contributor's branch: #632 and #634 act on `released`.
- A no sets `nos`, restarts the clock and marks the reply answered. The next `Quiet` asks again (`asks` 2). A no is not answered in public.
- A second ask met with silence is a release. A second no followed by `Quiet` raises the contribution ruling `waiting` (id the pull request's, no work item, never on the pull request or the panel, withdrawn with the pull request).

## The ruling

- `shep kelpie rule <pr> answer <text>`, text one of `keep waiting`, `adopt anyway`, `close with thanks` (or `keep`, `adopt`, `close`). Other text is refused with `RuleError::Choice` and the ruling stays. `src/runner/contributions/ruling.rs`
- `keep waiting` clears `asked` and restarts the clock. `adopt anyway` records `released { how: maintainer }` and posts nothing. `close with thanks` sets `asked.closing`; the next poll has the representative post thanks once, saves `thanked`, then calls `Forge::close_pull_request`, retried alone if it fails, so a restart does not thank twice. A push keeps an ask that is closing.
- State version 59. A file saved before it loads: `asked` and `released` are absent.
