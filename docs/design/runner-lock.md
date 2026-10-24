# The runner's lock

One `Mutex<Runner>` per project. #1201.

## What it covers

- Everything the runner knows: the state file, the work items, the forge and model ports. A pass takes it (`runner/trigger/lock`) and holds it through the calls it makes, which wait their turn behind the login's pace. The reads in `one_pass` before `begin` do not: see Off the lock.
- `status`, `drain` and `undrain` do not wait for it: `answer` tries the lock and, while a pass holds it, answers from the copy the loop last posted. Every other trigger waits. See `workers.md`.
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
