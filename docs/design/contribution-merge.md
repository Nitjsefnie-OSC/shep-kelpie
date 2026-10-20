# Contribution merge

Nobody but the maintainer merges someone else's work, so `git.merging = "auto"` does not reach a contribution. #630

## The ruling

- `follow_merge` (`runner/contributions/merge.rs`) runs once a poll, after `follow_clock`. A contribution whose pass has ended on the current head, whose CI is `green` on that head, and that has no `ruled` and is not being closed (`Contribution::awaits_merge_ruling`) gets one `contribution` ruling, reason `merge`: `Held::Merge { head }`, id the pull request, no work item. It names the findings and nits the pass recorded, and says the bot threads among them are still open when a review bot answered.
- Raised under `ask` and under `auto` alike. Nothing is merged, so no notice is sent: the ruling's alert is the only post. It is never posted on the pull request and never goes to the panel. `RulingName::ContributionMerge` is `contribution-merge`, and `rulings.pm_alone` naming it is refused at load. `crates/core/src/settings/rulings.rs`
- Options are `yes`, `no <note>` and `rework <note>` (`Wants::Merge`, the same three kelpie's own merge ruling takes). A `rework` takes the pull request over at once, without waiting for the contributor to release it (`contribution-release.md`). #634
- It holds nothing: the asks, the clock and the reviewers go on while it waits (`ruling_waits` and `over_budget` skip it). Any other ruling on the pull request takes its place and it is raised again once that is answered.
- A new head, or a closed, merged or unfollowed pull request, withdraws it with no post (`withdraw`), and the new head's pass raises another.

## The answer

- `apply` records it as `contributions[].ruled = { head, verdict }` (`rule_merge`) and the next poll carries it out, so a restart loses none of it. A new head clears `ruled`. Version 61.
- `verdict` is `{ yes: { merged?, thanked? } }`, `{ no: { note? } }` or `{ rework: { note? } }`. A no or a rework with no note, or the placeholder `NO_NOTE` a reaction, a tick or the card's choice gives, has no `note`: the worker is sent to the pull request and is told nothing made up.
- Yes: `Forge::merge(repo, number, head)` with the head the ruling named, so a head that moved is refused by the forge. A refusal is logged and tried at the next poll, and after `POST_TRIES` counted refusals the ruling is raised again; a busy or rate-limited forge is not counted. Then the representative posts thanks (`merged:<n>`). A call or a post that fails is logged, never holds the merge back and is not tried again, since the merged pull request leaves the record.
- No with a note: the representative words the note for the contributor and posts it (`declined:<n>`), then the note is cleared. No with no note posts nothing.
- Neither runs a fix turn. `ruled` stays until a push, so the same head is not asked about again.
- Rework merges nothing and posts nothing. The next poll records `released { how: rework }` (`take_it_over`), which is all adoption needs, and the same poll's `follow_adoption` queues the pull request for a work item (`contribution-release.md`). Its work item starts with a worker's turn and the project's full pass queued behind it (`Phase::Implement`, `resume: Review::first()`), not at the pass a release starts at: a clean pass over the contributor's own head would let the change the maintainer asked for go undone. Its note goes on `contributions[].rework` and not on `ruled`, so a contributor who pushes before the work item starts leaves it there: the answer is about the head it was given on, the note is about the pull request. The work item's start reads it into the adopted file, and it is repeated in the log. Version 66.
- A contribution with `released` is asked about merging no more: `raise_for` skips one, whatever its head does before its work item starts.
- The thanks and the note need the App (`app_covers`); without it nothing is posted.
- A maintainer's comment is `context` for the representative and does not reset the contributor's clock (`contribution-talk.md`).
