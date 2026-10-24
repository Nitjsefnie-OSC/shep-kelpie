# The runner's lock

One `Mutex<Runner>` per project. #1201.

## What it covers

- Everything the runner knows: the state file, the work items, the forge and model ports. A pass takes it (`runner/trigger/lock`) and holds it through the calls it makes, which wait their turn behind the login's pace. The reads in `one_pass` before `begin` do not: see Off the lock.
- `status`, `drain`, `undrain`, `timings` and `board` do not wait for it: `answer` tries the lock and, while a pass holds it, answers from the copy the loop last posted. The copy holds the status, the drain state, the board listing and the finished history. `board move` and `board top` write, so they wait.
- `add`, `rework`, `adopt`, `triage` and `rule` do not wait either. While a pass holds the lock, `answer` queues the verb on `Posted` and replies with the posted status, so a refusal is not in the reply. `rule` is checked against the posted rulings first: an id none waits on is refused in the reply. Every other trigger waits. See `workers.md`.
- Which calls run under the lock is unchanged by measuring it.

## What the metric shows

- `trigger::lock` and `answer` return a `Held` guard (`runner/lock_hold.rs`) that derefs to the `Runner`. On drop it records how long the lock was held, timed by `Instant`, against the `file:line` of the call that took it. `#[track_caller]` does the keying, so a function that calls `lock` must not carry it: every lock it takes would read as its caller's line.
- `Holds` keeps the longest hold per line, which `status` shows as `lock` (seconds, by `file:line`) and leaves out until one is recorded. The lines are the lock calls the code has, so the map stays that size.
- `send_metrics` sends what was recorded since its last send, in three series: `kelpie_runner_lock_held_seconds_total`, and `kelpie_runner_lock_holds_total{le="1"}` and `{le="+Inf"}`. `crates/core/src/metrics/lock.rs`.
- A line whose longest hold is long names the call to move off the lock. The sheep's own `lock()` calls (`src/sheep/`) are not timed.

## Off the lock

`runner/off_lock.rs`: `ItemKey` is a work item's `issue`, `session`, `turn`, `phase` and `pull_request` as they stood when the work began. `with_item` takes the lock, returns `None` if the item is gone or any of them changed, and otherwise sets `focus` to it again and applies the answer. 

These reads run with the lock let go, in `one_pass` before `begin`, each as a job that copies what it needs, reads, and applies on the lock:

- Main's CI (`main_ci/read.rs`): `still_red` is worked out again at the apply, so a workflow that went green meanwhile is not rerun.
- Parked items' pull requests and issues (`closed/read.rs`): one read per item, applied through `with_item`. `holds_work` still runs on the lock.
- Dropped tickets (`dispatch.rs`) and the bumps (`dependencies/read.rs`).
- Each stamps its `looks` or `parked_reads` entry at the apply, so a read that failed is made again at the next pass.
- A start-over pass makes none of them.

## Queued verbs

`runner/queued.rs`. `one_pass` takes the queue right after `take_posted`, so a verb waits for at most the pass that was running.

- A verb splits into a guard check, a read and an apply. `add`, `rework`, `adopt` and `triage` take the lock for the check, ask the forge with it let go (the issue, the pull request or the open issues), then take it again, check the guards once more and apply. Refusals read the same as the verb's own (`add_allowed` and `add_found`, `rework_allowed` and `rework_found`, `adopt_wanted` and `adopt_found`, `triage_allowed` and `triage_found`), with one more: `add` refuses an issue that is closed.
- A refusal goes to `Notes::standing` under the verb's key (`add 7`): it is logged once, lists in `status` under `standing`, and ends when that verb next goes through, queued or applied at once with the lock free (`trigger::answer` ends the key after a synchronous success). A queued `add` whose ticket already has an open work item is not a refusal: it did what it asked.
- `rule` applies under the lock: its forge reads sit inside `Runner::rule`, so they are not off the lock yet. A trigger that arrives meanwhile reads the posted copy.
- `rework` and `adopt` read the pull request off the lock. What `start_rework` and `adoptable` read after it (the viewer, the issue, the review bots, the branch's head) is still under the lock.
- A queued verb wakes the loop (`Posted::needs_wake`), as `drain` does.
- A queued verb lives only in memory, so a handover waits for the pass that takes it. `Handing::attempt` closes the queue under the runner lock (`Posted::close_queue`, which refuses while a verb waits) and opens it again once the exec is made or refused. A verb that finds the queue closed is not queued: `answer` waits for the lock, as every other trigger does across a handover.
