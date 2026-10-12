# A local round that reviewed nothing

- With the GPU box down (2026-10-01, while it was reinstalled), `qwen-review.sh` still wrote its `.done` marker and one `LOW|<file>:0|not reviewed: <why>|...` line per file, so kelpie read a finished round with a finding per file.
  - The judge rejected each, the round counted as clean, and a clean Claude round after it ended the loop on "two clean rounds from two different reviewers" with one reviewer having looked at anything.
  - It hit every item that day: 8 findings on each of #272's three local rounds, 3 and 3 on shep-pm/shep#668, 1 and 1 on shep-pm/shep#673.
  - The script's `why` is the first line of curl's or the model's error, `empty response`, or `off-format prose (<n> bytes)`.
- A file the script cannot say anything about leaves no line when it is reviewed clean, so a round's lines cannot tell "one file unreviewed, the rest clean" from "nothing reviewed".
  - Kelpie goes by the lines: a local round whose only lines are `not reviewed:` ones (the chunk-limit kind aside, which the hunk run handles and which stays a finding when it cannot) reviewed nothing. It sends the worker nothing, and the pass goes on to the next reviewer.
  - A second such round in a row, which takes a second pass, marks that local reviewer down for the work item: later passes go to the other reviewers, including any other local one, and `status` lists it under `local_reviewers_down`.
  - A round with real findings beside unreviewed files keeps the findings, and the report names the files; the work item keeps the list (`local_unreviewed`). A local round that leaves the same files unreviewed again counts as a failure against its reviewer, as a round that reviewed nothing does, and one that leaves none, or other ones, clears the count.
  - A hunk re-review that fails (a reboot mid-round left every hunk unreviewed on shep-pm/shep#674) is a `not reviewed:` line for its file, the same as the script's own.
