# The runner's lock

One `Mutex<Runner>` per project. #1201.

## What it covers

- Everything the runner knows: the state file, the work items, the forge and model ports. A pass takes it (`runner/trigger/lock`) and holds it through the calls it makes, forge reads included, which wait their turn behind the login's pace.
- `status`, `drain` and `undrain` do not wait for it: `answer` tries the lock and, while a pass holds it, answers from the copy the loop last posted. Every other trigger waits. See `workers.md`.
- Which calls run under the lock is unchanged by measuring it.

## What the metric shows

- `trigger::lock` and `answer` return a `Held` guard (`runner/lock_hold.rs`) that derefs to the `Runner`. On drop it records how long the lock was held, timed by `Instant`, against the `file:line` of the call that took it. `#[track_caller]` does the keying, so a function that calls `lock` must not carry it: every lock it takes would read as its caller's line.
- `Holds` keeps the longest hold per line, which `status` shows as `lock` (seconds, by `file:line`) and leaves out until one is recorded. The lines are the lock calls the code has, so the map stays that size.
- `send_metrics` sends what was recorded since its last send, in three series: `kelpie_runner_lock_held_seconds_total`, and `kelpie_runner_lock_holds_total{le="1"}` and `{le="+Inf"}`. `crates/core/src/metrics/lock.rs`.
- A line whose longest hold is long names the call to move off the lock. The sheep's own `lock()` calls (`src/sheep/`) are not timed.

## Off the lock

`runner/off_lock.rs`: `ItemKey` is a work item's `issue`, `session`, `turn`, `phase` and `pull_request` as they stood when the work began. `with_item` takes the lock, returns `None` if the item is gone or any of them changed, and otherwise sets `focus` to it again and applies the answer. Nothing calls it yet.

The triage poll (`runner/triage/poll_read.rs`, #1204) has the same shape without a work item. `read_triage` runs in `one_pass` before the pass takes the lock, in three parts like `thread_replies`:

- The job is built under the lock: `Runner::poll_job` returns none unless `triage.pick` is not `off` and the look is due (`Looks::triage_due`, which stamps nothing), and copies the ports, the local waits' issue numbers and what the pulls need.
- The run lists the open issues, reads each local wait's issue, and lists the open pulls only where one is used: a listed issue carries `ready-for-agent`, a recorded taken-back one is listed, or the triager may be started.
- The apply re-takes the lock, stamps the look (`triage_listed`), and does every write from the state it finds then: `note_triage_authors`, `settle_triage_waits`, `resume_triage`, the take-back's label writes and `save_taken`. A failed listing stamps nothing, so the next pass reads again, at most once a second.
- The listing and pulls stay as `triage_run.listing`, the snapshot `triage_job` starts the triager from. It is at most one poll old, and what a round has since changed in the records is checked against the state, not the snapshot.
- Still under the lock: the reads in `triage_ask` (the `triage` trigger) and in `ask.rs`, `comment.rs`, `limits.rs` and `addressed.rs`.
