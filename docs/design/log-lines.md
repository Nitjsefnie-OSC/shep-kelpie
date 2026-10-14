# Log lines

What the runner's out log (stdout and stderr) and the dog's log say about what kelpie is doing between steps. A step is one line, `#<issue> <name> key=value` (`workers.md`); the lines here are notes, which `src/sheep.rs` prints each pass. #854

## Runner

- Pause: `drain pause` notes the issues whose turn it asked to end at its next tool call, or that none could. A turn that ends that way notes `#<n> paused-at-tool`, and the start that resumes it from `PAUSED` notes `#<n> resumed-after-pause`, once. A paused turn still reports no step. `src/runner/drain.rs`, `src/runner/turn/paused.rs`
- Handover: the new image prints `took over: <project>, from kelpie <old> to kelpie <new>, <n> turns carried, ...` in place of `up:`. `Carry` holds the old build's version (`build`, absent from an image that predates it, which reads `from an earlier build`). A restart still prints `up:`. `src/sheep/handover.rs`
- Lease wait: the first pass a review round is not granted its bot's lease notes `#<n> lease-wait kind=<lease> reviewer=<name>`; later passes say nothing. The grant notes `#<n> lease-granted kind= reviewer= waited=<s>s`. A round that gives up on the lease (no window, or `REVIEW_WAIT`) notes `lease-refused` the same way, before its `reviewer-skipped` step. A grant at the first ask says nothing. The start is in memory, so a restart says the wait again. `src/runner/review_bot/lease_wait.rs`
- Board skip: a ready issue the board passes over notes `#<n> skipped reason=<why>` when the skip begins or its reason changes, and `#<n> skip-cleared` when it ends, on any pass, whether or not one dispatches. The reason is the briefing's. After a restart each standing skip is said once more. `src/runner/skip_log.rs`
- Standing warning: a warning that holds while its cause does is noted on the first call that meets it and not again until the runner starts, and `status` lists it under `standing` while it holds. The code graph server missing (ends when it is installed) and its index failing (ends at the next index that works); the App's token lacking a permission, once for each repo and permission whichever read met it. A call ended while it indexes is a plain note. `src/runner/claim.rs`, `src/runner/code_graph.rs`, `src/ports/rate_limit/app_reads.rs`

## Dog

- A grant of a build, test or e2e lease is logged only when the ask had to queue, with how many were ahead; a refusal is always logged. `src/dog/door.rs`
- An ask the dog does not act on is logged once for each kind, direction and total in a run: `ignored <kind> want|return total <n> from <sheep> run <epoch>: the dog recorded <m> for this run`. The runner re-sends its total each pass while it waits, and an equal total while the dog has it asking is the normal case, so it is silent. `src/dog/desk.rs`
