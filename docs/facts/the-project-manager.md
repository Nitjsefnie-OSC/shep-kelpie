# The project manager

- Measured 2026-10-05, on 33 decision points from shep and shep-kelpie and a scripted day of 40 wakes on shep-kelpie:
  - The board rule got 5 of 18 picks and holds right and avoided 4 of 15 avoidable conflicts, where every PM arm avoided 9 to 12 of 11 to 15, since most ready issues name no file and the rule cannot see a conflict.
  - Opus 5.5 at medium, briefed from the board, got 27.5 of 33 decisions right, avoided 10 of 15 and 9 of 14 in two runs, at 28,914 units ($0.114) and 12.9 seconds a wake; the same model looking for itself with git and a stand-in `gh` got 28 at 53,919 units ($0.197) and 25.9 seconds.
  - Units weigh cache reads 0.1, cache writes 2, output 5 and input 1.
- Counted by the maintainer over one day's kelpie log (shep-pm/shep-kelpie#987): 71 of 134 project manager wakes began less than 5 minutes after that project's previous wake, and each wake pays for its whole starting context, median 54k tokens. Holding the reasons nobody waits on for 3 minutes sends those in one call.
- The session shape on the day, Opus at medium:
  - A fresh session every wake cost 52,891 units ($0.204) a wake, $8.16 a day.
  - One resumed and replaced by a fresh one past 100k cost 35,585 ($0.106), $4.23.
  - One kept all day and compacted past 100k cost 28,917 ($0.083), $3.31, and answered all 15 probes of what it knew in both runs.
  - A board that day ran 17k to 30k characters, which a fresh session writes to cache on every wake.
- A resumed session's context reached 100k after 14 to 27 wakes. One `/compact` cost 25k to 36k units, 65k to 108k with the wake after it, and left 9k to 13k of context. A fresh session's first wake after a resumed one cost 41k to 73k.
- A fresh Opus session that did not write the maintainer's `tell` into its notes missed it at 3 of 8 checkpoints; every resumed or compacted session kept it.
- The measured PM missed a worker hung on a prompt in 7 of 8 tries, because the board said only "busy".
- Sonnet 5.5 at high decided as well as Opus 5.5 at medium at about half the dollars a wake from a fresh session, but in the session shape kelpie runs, one compacted as it goes, it spent more tokens and cost about the same ($0.079 against $0.083 a wake).
- Every arm unstuck 12 or 13 of 15 stuck items right. Nearly all missed a worker hung on a permission prompt for an hour while its status read "busy", which the board's idle line now shows, and a review loop whose findings narrowed each round, which the maintainer kept running.
- `claude -p --output-format json` reports `usage` summed over the call's model requests, and `usage.iterations` holds the last request's own: a recorded eight-turn result reads 108,226 cache-read tokens in its sum and 30,304 in its one iteration. The context a session holds is that iteration's input, cache writes and cache reads.
