# No reviewer reads a head until CI has passed on it

This amends ADR 0006, which runs the review pass first and sends "whatever the last fix leaves" to CI. Its readers stand: each listed reviewer reads once, in order, with no loop and no judge. What changes is when a reviewer may start. On a project's pull request on 2026-10-10:

- The worker pushed, and CI failed in 1.5 minutes.
- The local reviewer spent 8 minutes reading that red head and sent 4 findings.
- The fix turn pushed a head that was red again, and kelpie summoned a review bot on it.

A bot's window is scarce, a model's read costs real money, and a red run is a sure defect that costs neither. The maintainer decided on 2026-10-10 that the order flips: no reviewer reads a head until it is pushed and green.

- **The wait.** Before a reviewer starts, or a bot is summoned, kelpie waits for CI on the head that reviewer would read: the worker's push, and the head of each fix turn after it. A run reads as it does at the gate: two minutes after kelpie first saw the head, since a check set registers a check at a time. Pending waits, and green goes on. A head green for one reviewer is not waited on again for the next while it stays the head, and a fix turn that pushed nothing leaves it so.
- **A red run** is the worker's CI fix turn before any reviewer reads, naming the failed checks, and it counts toward `ci.fix_attempts` as a red run after the review did. Its fix comes back to the review, not on to CI and the merge, and waits for green in turn. A head still red after a fix turn that pushed nothing, or a red run once the cap is reached, is the `still-red` ruling as before.
- **A nit-only fix** (ADR 0007) waits for green too before the pass goes on: the next reviewer reads the nit fix's head only once its run has passed.
- **With `ci.block` off** the review starts at once, as before.
- **The draft pull request needs CI to run while it is a draft.** A repo whose workflows all skip drafts would hold every review for a run that never comes, so `doctor` names it (`ci on drafts`) and says to let one workflow run on drafts or turn `ci.block` off. It reads the conditions GitHub's documentation shows, `github.event.pull_request.draft == false`, `!= true` and a leading `!`, and a `pull_request` `types` list with none of `opened`, `synchronize` and `reopened`. A workflow counts as skipping if its trigger never fires on a draft, or every one of its jobs has the condition, and no push runs it. A draft whose rollup is nothing but skipped runs is not green either: the round waits, as it would for a run that never comes, and a ready pull request with such a rollup passes, as the gate has always passed it (`Checks::Skipped`). A repo with any pull request workflow that does not skip is not named. A bot that skips drafts marks the draft ready for itself, as before (#65), and it does so only once the head is green.
- **Time** is charged as before: a round waiting for its run is `ci`, the reviewer's read is `review`, and the six phases still sum to the item's wall time.

After the pass the work item still goes to CI, which finds the head already green unless a late push moved it, and then raises the merge ruling or merges.

## Considered options

- **Keep the review first.** It is ADR 0006's order, and it read 8 minutes of a head that was sure to be fixed, then summoned a bot on a second red head.
- **Run CI and the first reviewer at once.** The first read is the costliest and its findings are the likeliest to be made moot by a fix to a red run, so the saving is small and the waste is the case above.
- **Skip the wait for a local reviewer, since it costs no bot window.** It still costs the machine's minutes, and the fix turn a red run needs would make its findings stale.

## Consequences

- A review starts later by however long CI takes, at least the two minutes a run needs to register. That time is `ci`, and it was spent after the review before.
- A `still-red` ruling raised before the review leaves the head unread. A yes looks again at CI, and a green run then raises the merge ruling naming the head as one no reviewer read, so the maintainer decides on it.
- The state file is version 23: a review round may carry `ci`, the run it waits on. A version 22 file loads with none, and an older kelpie refuses a version 23 file.
- A pull request adopted with a review bot owed waits for green before the bot is summoned.
