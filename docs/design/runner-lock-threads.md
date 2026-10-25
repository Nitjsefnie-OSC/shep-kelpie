# Trigger threads

Each trigger runs on a handler thread of its own since shep 0.17. #1288, part of #1201. The rest of the lock: runner-lock.md.

## Handlers

- shep-channel runs each action handler on a thread, up to 32 at once. A trigger that waits for the runner's lock holds only its own thread, so a `status` sent after it is not held behind it.
- The posted copies stay. `status` would wait on the same lock, past shep's `action_timeout`.
- `status_sent_after_a_trigger_that_waits_for_the_pass_is_answered_at_once` in `crates/runner/src/runner/trigger/tests/mod.rs`: `finish` waits on a held lock on one thread, and a `status` on another is answered at once.
