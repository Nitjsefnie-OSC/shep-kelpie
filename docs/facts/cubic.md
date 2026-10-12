# cubic

Read 2026-09-29 from cubic's answers on shep#637, #645, shep-kelpie#106 and #116, and shep-openbao#4. shep#637's comments, review and threads and shep#645's review are recorded in `fixtures/`.

- The free plan gives a repo GitHub marks private 20 reviews a month. A public repo draws on a fair-use pool of reviewed lines it does not publish. shep-kelpie#116's review footer said "You're at about 91% of the monthly reviewed-line limit", so the pool is counted by the month.
- `@cubic-dev-ai review` in a comment summons it. On shep#637 the maintainer's summon at 12:15:54Z drew an issue comment from `cubic-dev-ai[bot]` at 12:16:00Z quoting the summon, "I have started the AI code review. It will take a few minutes to complete.", and the review of d51bb92 at 12:23:15Z.
- It sets no commit status. It runs a check run, "cubic · AI code reviewer", from 12:16:04Z to 12:23:28Z on shep#637, which kelpie does not read.
- Every review posts a review object on the commit it read, a clean one included: shep#645's says "**No issues found** across 8 files". The count sits between `<!-- cubic:review-summary:start -->` and `:end -->`.
- Each finding is a review thread whose first comment carries `<!-- metadata:{"confidence":N} -->`, N out of 10, then a line opening with its level, `P2:` or `P3:` on shep#637 (4 and 7 of 11), then a collapsed prompt for agents and a "Fix with cubic" link. No P1 was seen.
- On shep-openbao#4 it started an "ultrareview" on its own, announced in a comment, and posted a second review of the same commit when it ended, 11 minutes after it began.
- A refusal was never seen, and cubic does not document its wording. Kelpie reads any cubic comment after the summon that names a limit reached or exceeded as one, with no opening.
- Kelpie's `cubic` agent file holds the free plan's window for a repo GitHub marks private, 20 reviews in 720 hours (shep-pm/shep-kelpie#315). A refusal quotes no opening, so it holds cubic for that span, which is more than an hour, and the pass goes on without it.
