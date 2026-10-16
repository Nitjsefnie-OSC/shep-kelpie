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

- `shep kelpie rule <pr> answer <text>`, text one of `keep waiting`, `adopt anyway`, `close with thanks` (or `keep`, `adopt`, `close`); for `no-edits` (below), `leave it open` or `close with thanks` (or `leave`, `close`). Other text is refused with `RuleError::Choice` and the ruling stays. `src/runner/contributions/ruling.rs`
- `keep waiting` clears `asked` and restarts the clock. `adopt anyway` records `released { how: maintainer }` and posts nothing. `close with thanks` sets `asked.closing`; the next poll has the representative post thanks once, saves `thanked`, then calls `Forge::close_pull_request`, retried alone if it fails, so a restart does not thank twice. A push keeps an ask that is closing.
- State version 59. A file saved before it loads: `asked` and `released` are absent.

## Adopting a released one

- `adoptable` (`src/runner/adopt.rs`) lifts `AdoptError::Fork` and `AdoptError::Author` for a pull request whose record has `released`. Any other pull request is refused as before. #632
- `Reviewed` carries `maintainer_can_edit` (`maintainerCanModify`) and `head_clone_url` (`headRepository.url` plus `.git`), read in `src/adapters/gh/review.rs`. `Reviewed::pushable` is true for a branch in the repo itself, and for a fork only with edits and a URL. A pull request that is not `pushable` is refused as `AdoptError::NoEdits`.
- A fork is adopted onto local branch `kelpie/<issue>` (its own name could be `main`), with `remote: Fork { url, branch }` on the work item and the worktree at the fork's head. A branch in the repo itself is adopted under its own name on `Origin` and needs no edit permission. The catch-up with `main`, the foreign-change and regate adoptions and the agents' own files check use the work item's `remote`. The worker's own push (`OPEN_YOURSELF`, still `git push origin HEAD`) is #633, the issue and the pass #634.
- `Runner::awaits_edits` (`src/runner/contributions/adopt.rs`) runs in `release_one` where an open ask meets `Quiet` with no reply, after replies are read; an ask first made at `Quiet` is made as before. On a fork that is not `pushable` it raises `Held::NoEdits` (`reason: no-edits`) and releases nothing; an unreadable pull request waits for the next poll. Nothing is posted on the pull request.
- `leave it open` pushes the number to `ProjectState.left_open`: `is_contribution` skips it, the record drops with its turns, and nothing is posted. An entry goes when the pull request leaves the open listing. `close with thanks` is as for `waiting`. State version 63.

## Remote

- `worktree::Remote` is where a work item's branch lives: `Origin`, or `Fork { url, branch }` for a contributor's fork. `prepare` (`Start::Pushed`), `origin_head`, `base_of`, `adopt`, `rebase` and `remove` take it. A work item records it as `remote`, left out for `Origin`; adopting a released fork contribution sets `Fork` (`contribution-release.md`), and every other caller passes `Origin`. A fork is fetched and pushed by URL, never added as a remote of the repo. `src/worktree/remote.rs`
- A fork's branch is fetched to `refs/remotes/fork-<hash of the URL>/<branch>`, never under `origin`, so a fork cannot overwrite `origin/main`. `Start::Main` and the catch-up with `main` always read `origin/main`. #631
