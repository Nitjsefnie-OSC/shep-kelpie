# The runner's lock

One `Mutex<Runner>` per project. #1201.

## What it covers

- Everything the runner knows: the state file, the work items, the forge and model ports. A pass takes it (`runner/trigger/lock`) and holds it through the calls it makes, which wait behind the login's pace, except the reads in Off the lock and The rotation.
- `status`, `drain`, `undrain`, `timings` and `board` do not wait: while a pass holds the lock, `answer` replies from the copy the loop last posted (status, drain state, board listing, finished history). `board move` and `board top` write, so they wait.
- `add`, `rework`, `adopt`, `triage` and `rule` do not wait either: `answer` queues the verb on `Posted` and replies with the posted status, so a refusal is not in the reply. `rule` is checked against the posted rulings first: an id none waits on is refused in the reply. Every other trigger waits.

## What the metric shows

- `trigger::lock` and `answer` return a `Held` guard (`runner/lock_hold.rs`) that derefs to the `Runner`. On drop it records the hold against the `file:line` of the call that took it (`#[track_caller]`, so a function that calls `lock` must not carry it).
- `Holds` keeps the longest hold per line, which `status` shows as `lock` (seconds, by `file:line`).
- `send_metrics` sends what was recorded since its last send: `kelpie_runner_lock_held_seconds_total` and `kelpie_runner_lock_holds_total{le="1"}` and `{le="+Inf"}` (`crates/core/src/metrics/lock.rs`).
- A long hold names the call to move off the lock. The sheep's own `lock()` calls (`src/sheep/`) are not timed.

## Off the lock

`runner/off_lock.rs`: `ItemKey` is a work item's `issue`, `session`, `turn`, `phase` and `pull_request` when the work began. `with_item` takes the lock, returns `None` if the item is gone or any changed, and otherwise sets `focus` to it and applies the answer.

These run with the lock let go in `one_pass` before the rotation, each a job that copies what it needs, reads, and applies on the lock:

- Main's CI (`main_ci/read.rs`): `still_red` is worked out again at the apply.
- Parked items' pull requests and issues (`closed/read.rs`), one read per item.
- Dropped tickets (`dispatch/mod.rs`) and the bumps (`dependencies/read.rs`).
- Each stamps its `looks` or `parked_reads` entry at the apply, so a failed read is made again next pass.

The triage poll (`runner/triage/poll_read.rs`, #1204) has the same shape without a work item:

- `Runner::triage_poll_job` returns none unless `triage.pick` is not `off` and the look is due (`Looks::triage_due`). The run lists the open issues, reads each local wait's issue, and lists the open pulls only where one is used.
- The apply stamps the look (`triage_listed`) and does every write from the state it finds then. A failed listing stamps nothing.
- The listing and pulls stay as `triage_run.listing`, which `triage_job` starts the triager from. A `triage` verb sent during the read (`Looks::triage_asked`) makes the apply keep none and stamp nothing.
- Still on the lock: the reads in `ask.rs`, `comment.rs`, `limits.rs` and `addressed.rs`.

## Queued verbs

`runner/queued.rs`. `one_pass` takes the queue right after `take_posted`, so a verb waits for at most the pass that was running.

- A verb splits into a guard check, a read and an apply. `add`, `rework`, `adopt` and `triage` check under the lock, ask the forge with it let go (the issue, the pull request or the open issues), then take it again, check the guards once more and apply. Refusals read as the verb's own (`add_allowed`, `add_found` and the like), plus one: `add` refuses a closed issue.
- A refusal goes to `Notes::standing` under the verb's key (`add 7`): logged once, listed in `status` under `standing`, and ended when that verb next goes through. A queued `add` whose ticket has an open work item did what it asked.
- `rule` applies under the lock: its forge reads sit inside `Runner::rule`. `rework` and `adopt` read the pull request off the lock, but `start_rework` and `adoptable` still read the viewer, the issue, the review bots and the branch's head under it.
- A queued verb wakes the loop (`Posted::needs_wake`).
- A queued verb lives only in memory, so a handover waits for the pass that takes it. `Handing::attempt` closes the queue under the lock (`Posted::close_queue`, which refuses while a verb waits) and opens it once the exec is made or refused. A verb that finds it closed waits for the lock.

## The board read

`runner/board_read.rs`: `read_board` runs in `one_pass` after the reads above and before the rotation. `poll_job` takes the lock and, if the board may open a work item, its look is due and the forge is not held, copies what the read needs into a `BoardJob`. `BoardJob::run` reads with the lock let go. `store_board_read` takes the lock again and keeps the result for the pass.

- The job reads in the old order: the ready issues, the open pull requests, the `issue` of each finished ticket the board no longer lists, then the parents when none is left to close. A failed read ends the rest. `forge.jsonl` is unchanged.
- The open-pull-request listing goes into the `Snapshot` as a preload `begin_pass` keeps, so the gate, merge and rework steps serve from it.
- `dispatch` takes the stored read, then stamps the look (`reading_board`). A failed read stamps nothing and `stepped` retries the slot in 15 seconds, doubling to 10 minutes, as `main_ci_read` does. `trim_finished`, `close_done_parent`, `close_issue` and every save stay on the lock.
- A read the pass did not use is dropped at `end_pass` and stamps nothing: at most one wasted read a poll.
- A read is also dropped, with its listing, when a write of kelpie's came after it (`forge_ledger.writes()` changed). A board that opens after the pre-pass found it not due reads in place.
- It follows `main`'s CI, the dropped tickets and the bumps, so they learn a used-up limit first, and `poll_job` reads nothing while the forge is held.

## The rotation

`runner/flight/rotation.rs`: `begin_rotation(&Mutex<Runner>)` takes the lock once to open the pass and plan it, then once for each slot's step and the call it starts. `snapshot.end_pass()` runs however it ends.

- A step that needs a read returns `Begin::Reads(job)` (`turn/reads.rs`), built from cloned `Arc`s, owned ids and an `ItemKey` (`Snapshot` is `RefCell`, so `Runner` is not `Sync`). The rotation lets go, runs it, and takes the lock with `with_item`.
- A changed or dropped item gives an idle slot. Otherwise the answer goes into the pass's `Reads`, errors as the inline call gave them, and the step is made again, taking the answer or reading in place. State is read after the gap and drain is checked then, so a drain that lands in the gap starts no call.
- Reads that ship: the pull request at the top of `check_ci`, which lists the open pulls first when the pass has listed none; the review head (`review/reads.rs`); a bot's activity (`review_bot/bot_reads.rs`); `talk_turn`'s comments; the gate's and merge's (`gate/reads.rs`): the pull request, the failed logs, the queue standing; the merge write stays on the lock. `talk_turn` tells a failed read once.
- A turn's start (`Read::Start`, #1210) is one job: the worker token, the first prompt's issue, then the maintainer's comments, each only where the turn takes it, and none after a failed one. `start_job` is asked once the drain, the token wait and the slot let the turn start, so a turn that does not start mints no token. A token lasts an hour, so one kept across the gap is safe. Worktree `prepare` (git) and every write stay on the lock.
- The project manager's wake (`pm/ci_logs.rs`, #1210) reads `failed_run_logs` per still-red item and `run_log_tail` per red workflow in one job, and writes the files under the lock from its result. `pm_ended` and `shep kelpie pm` wakes read in place. A step asks for what it needs, and its reads are kept until it ends, so a step with several misses is made again over the earlier ones.
- `origin_head`, `labelled`, `visibility` and the `viewer` still read on the lock.
- `turn_ended` returns `Ended::StartOver(ticket)`, and `hear_news` runs `rotation::start_over` after it drops the lock.
