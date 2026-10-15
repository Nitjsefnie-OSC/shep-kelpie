# Contribution merge

Nobody but the maintainer merges someone else's work, so `git.merging = "auto"` does not reach a contribution. #630

## The ruling

- `follow_merge` (`src/runner/contributions/merge.rs`) runs once a poll, after `follow_clock`. A contribution whose pass has ended on the current head, whose CI is `green` on that head, and that has no `ruled` and is not being closed (`Contribution::awaits_merge_ruling`) gets one `contribution` ruling, reason `merge`: `Held::Merge { head }`, id the pull request, no work item. It names the findings and nits the pass recorded, and says the bot threads among them are still open when a review bot answered.
- Raised under `ask` and under `auto` alike. Nothing is merged, so no notice is sent: the ruling's alert is the only post. It is never posted on the pull request and never goes to the panel. `RulingName::ContributionMerge` is `contribution-merge`, and `rulings.pm_alone` naming it is refused at load. `src/settings/rulings.rs`
- Options are `yes` and `no <note>` (`Wants::YesOrNo`). #634 adds `rework`.
- It holds nothing: the asks, the clock and the reviewers go on while it waits (`ruling_waits` and `over_budget` skip it). Any other ruling on the pull request takes its place and it is raised again once that is answered.
- A new head, or a closed, merged or unfollowed pull request, withdraws it with no post (`withdraw`), and the new head's pass raises another.

## The answer

- `apply` records it as `contributions[].ruled = { head, verdict }` (`rule_merge`) and the next poll carries it out, so a restart loses none of it. A new head clears `ruled`. Version 61.
- `verdict` is `{ yes: { merged?, thanked? } }` or `{ no: { note? } }`. A no with no note, or the placeholder `NO_NOTE` a reaction or a tick gives, has no `note`.
- Yes: `Forge::merge(repo, number, head)` with the head the ruling named, so a head that moved is refused by the forge. A refusal is logged and tried at the next poll, and after `POST_TRIES` the ruling is raised again. Then the representative posts thanks (`merged:<n>`). A call or a post that fails is logged and never holds the merge back.
- No with a note: the representative words the note for the contributor and posts it (`declined:<n>`), then the note is cleared. No with no note posts nothing.
- Neither runs a fix turn. `ruled` stays until a push, so the same head is not asked about again.
- The thanks and the note need the App (`app_covers`); without it nothing is posted.
- A maintainer's comment is `context` for the representative and does not reset the contributor's clock (`contribution-talk.md`).
