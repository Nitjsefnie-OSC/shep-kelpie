# The first hands-on run

Measured 2026-09-28 and 2026-09-29 on kelpie-scratch (shep-pm/kelpie-scratch, public), with the runner under `~/.kelpie/shep` and builds from f738abd to e1dbe90. Five runs for #17.

- A labelled issue reached a merged pull request through every gate: issue #1, pull request #3, merged as 4b6d2d3 at 21:21Z. The worker's turn ran 20:52 to 20:54Z, qwen's round 1 and Claude's round 2 came back clean, CI passed all 7 checks on ubuntu and macos, one CodeRabbit round was satisfied, and the merge ruling got a yes. Run 2 (issue #4, pull request #5, merged as f0723de at 22:12Z) and run 4 (issue #8, pull request #9, merged as eb15925) did the same.
- CodeRabbit skips a draft that carries the label: "Draft PR not reviewed" at the 20:59:52Z summon. A hand `@coderabbitai review` comment inside kelpie's lease got past it. #65 marks the pull request ready before the CodeRabbit round.
  - Run 3 (issue #6, pull request #7) showed it live: ready at 22:42:08Z, before any summon, then the lease waited for the window (the last summon was 22:04:19Z), the dog granted at 23:04:18Z, the label went on at 23:04:23Z, and no draft was skipped.
  - Run 4 was marked ready at 23:38:50Z and summoned the moment the window opened, at 00:05:52Z, with no refusal.
- A cold cargo cache works inside the worker's sandbox. With `build_env` `CARGO_HOME = "cargo"` (inside the build folder), build, clippy and tests passed in the worker and in CI, and the worker needed no paths or domains beyond crates.io's three.
- Removing `review please` stops a later push from summoning. The label came off as each round was satisfied (21:04Z and 22:07:27Z).
  - A commit pushed by hand at 22:08:03Z, with the label off, made CodeRabbit update its comment at 22:10:39Z to "Review skipped: Auto reviews are limited based on label configuration", and it posted no review.
  - That commit reached the merge ruling with CI only, with no review loop and no CodeRabbit round, because #16 watches labels and ready state and nothing else. Filed as #69.
- The runner waited at stage `lease` until the `kelpie dog` app was added under kelpie's shepherd, which was granted within seconds. Filed in #64.
- Run 5 (issue #10) of the first hands-on run closed the question loop: the worker asked at 00:59:28Z (ruling 6), the maintainer's answer reached the runner over the shepherd's socket at about 01:02Z, and the worker built on it.
- The webhook (ntfy) delivered the alerts for ruling 1 (21:06Z) and ruling 2 (21:31Z) to the maintainer's phone, confirmed by the maintainer.
- Cost by role: before run 2's merge, `status` showed 2 calls, all the worker's, since Claude review rounds and judge calls were not recorded. Filed as #70. Run 4's `status` before its merge did show it: worker 1 call, review 1 call, judge 0 calls, and one qwen round of 1131 s, mostly the GPU queue.
- CodeRabbit's refusal path (#12) ran live in run 3. The hour counted from the hand comment's summon at 22:04:28Z.
  - Kelpie took the label off at 23:05:28Z and summoned again at 23:05:36Z. It read the same refusal, whose quoted time (23:04:44Z) was already past (`summon-refused` at 23:05:39Z), and summoned a third time at 23:05:46Z, which landed. That cost one wasted label toggle from re-reading a stale refusal.
  - CodeRabbit reviewed at 23:10:57Z with one thread, the judge did not hold it (low), and one round satisfied it.
- Kelpie's 0.10.1 shepherd was replaced by a 0.8.2 one (`~/.cargo/bin/shep`, first on `PATH`) on the same home twice on 2026-09-28, at 19:16 and again at 21:01 local, and the old shepherd's children survived as orphans: a second webapp runner and a second dispatcher.
  - The second time, `shep trigger` with the 0.8.2 shep got a version-skew error that said "Run: shep daemon reload", and running that re-executed the older binary and orphaned the runners. The first takeover most likely went the same way.
  - Each time the orphans were killed and the shepherd was restarted on 0.10.1 by hand.
