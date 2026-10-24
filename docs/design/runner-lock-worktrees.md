# Worktree jobs

The calls that change a work item's worktree run with the runner's lock let go. #1212, part of #1201. The rest of the lock: runner-lock.md.

## Jobs

- Three: the rebase (`gate/catch_up.rs`), the cleanup of a finished item (`merge/cleanup.rs`) and the worker's worktree (`git_reads.rs`, `Preparing`). Each copies what it needs into owned values and runs as a job (`turn/reads.rs`), like the reads before it.
- Rebase and removal continue from the answer (`worktree_jobs.rs`, `Resume::Applied` in `flight/rotation.rs`): the apply runs the rest of the step, so a push and its save keep their order and nothing is read again. A step made again would compare the moved worktree with a head read before the push.
- Preparation is kept like a git read: the step is made again, `worker_reach` takes the kept worktree, and a `drain` that landed meanwhile is checked again before the call starts.
- The apply re-checks the `ItemKey`. A rebase whose item changed is not saved, and the next pass reads the head again. A failed call reaches the step as the inline call returned it.
- Cleanup runs the forge writes and releases first, on the lock, then removes. `drop_work_item` removes in place.

## Busy

- `off_lock::Busy` records the issue whose worktree a job changes, from the step that returns the job to its apply.
- `drop` sent meanwhile marks the item and replies as it does when it succeeds. At the apply, the answer is dropped and `drop_work_item` runs. A refusal then stands as a note under `drop N`. A removal's own apply runs and discards the mark.
- `rework` and `adopt` queue like `add` (`Posted::ask_queued`) while any job runs. `attach` answers `Waiting` for the busy item, which the client polls.

## Still on the lock

- Forge writes and their saves, and `contributions/`.
- `start_adoption`'s remove and prepare: the item does not exist yet, so a job has no key.
- `worker_reach` from the review checks, the panel and attach, which have no step to make again.
- `friction_of`'s read of the item's journal, and the worker token's mint in `prepared`.
