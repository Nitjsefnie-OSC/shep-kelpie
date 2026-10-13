# The qwen GPU lock

Kelpie no longer holds or takes this lock (#502): paddock queues the GPU. What follows is what was measured while it did, and it still describes the qwen scripts.

- `qwen-review.sh` builds its GPU lock from `${TMPDIR:-/tmp}/qwen-review/gpu.lock`, so the lock's path depends on `TMPDIR`. An interactive Claude session gets the per-user temp folder as `TMPDIR` (`getconf DARWIN_USER_TEMP_DIR`). A runner under shep gets none, so it locked `/tmp` and its rounds ran on the GPU beside the sessions' rounds. Another session's model calls hit the 1800s timeout, seen on the playground on 2026-09-28.
- Killing a qwen round takes 20 to 70 seconds and throws the partial round away, so a running round is never preempted.
- `kelpie lease run gpu` used to lose an interrupt that landed between taking the lock and spawning the command: the terminal's interrupt reached kelpie but no child, and the handler had nothing to forward it to, so the run went on and only a second Ctrl-C ended it.
  - With a 60 ms pause added in that gap, `run_removes_the_lock_when_interrupted` failed 20 of 20 runs; unchanged `main` passed 1,110 runs, so it is rare. Measured 2026-09-29 with the same pause and a probe that interrupts as soon as the lock exists: 0 of 20 pass before the fix, 20 of 20 after.
  - A signal that has arrived by the time `run_command` starts now ends the run, exit code 128 plus its number, and the lock goes back. One window is left: a signal between that check and the child's fork still reaches no one, microseconds wide.
- That check missed a signal the OS had handed kelpie but tokio had not yet flagged: tokio flags a signal from its driver, not from the handler, so a `recv()` polled in that gap is pending.
  - With the one-second pause that hid the gap taken out of the unit test, the command ran and the run exited 0, every time on a current-thread runtime.
  - Every pre-spawn check (`run_command`, so `run_gpu` and `run_book`, and the counted run) now yields once before it asks the streams (`Signals::already`), so the driver has caught up. The gap after that stays microseconds wide.
  - Under 24 busy processes on 2026-10-11, unchanged `main` passed `run_removes_the_lock_when_interrupted` 130 of 130, so the integration test's failure was not reproduced here.
- Every qwen round now runs with `TMPDIR` set to the folder the dog's own lock is under (`lease::gpu::temp_dir`: `TMPDIR` if set, else `getconf DARWIN_USER_TEMP_DIR`, else `/tmp`), so kelpie and the sessions queue on one lock.
