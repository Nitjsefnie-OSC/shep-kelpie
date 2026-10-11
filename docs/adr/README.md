# Architecture decision records

Each ADR below is one decision that is hard to reverse. A later ADR can reopen part of an earlier one, and says so in its own text.

## Decisions

- 0001 [Kelpie stands on shep, not in it](0001-kelpie-stands-on-shep-not-in-it.md): kelpie is its own repo built on the published shep crates, and a generic gap becomes a shep issue.
- 0002 [Projects are sheep, workers are lambs](0002-projects-are-sheep-workers-are-lambs.md): each project runs as one long-lived sheep and its workers are that sheep's lambs.
- 0003 [Runners live in the maintainer's own shepherd](0003-runners-live-in-the-maintainer-s-own-shepherd.md): runners and the dog are sheep of the maintainer's own shepherd, and kelpie is adopted as a dog.
- 0004 [The adopted kelpie is the lease dog](0004-the-adopted-kelpie-is-the-lease-dog.md): the adopted `kelpie` holds the leases through the shepherd channel shep 0.12 gives it, and `kelpie-dog` is gone.
- 0005 [shep stops every lamb](0005-shep-stops-every-lamb.md): shep's stop reaches every lamb in the tree, and kelpie drops its own stop ladder, orphan sweep and pid files.
- 0006 [One agent builds, reviewers run once in series](0006-one-agent-builds-reviewers-run-once.md): the issue says what done means, one agent builds it, and each listed reviewer reads once with one fix turn, with no loop and no judge.
- 0007 [Review rules for a live pull request](0007-review-rules-for-a-live-pull-request.md): nits and a merge ruling's `no` get a fix turn, a late bot is read, and a parked item frees its slot.
- 0008 [No reviewer reads a head until CI has passed on it](0008-no-reviewer-reads-a-head-until-ci-has-passed-on-it.md): a reviewer starts, or a bot is summoned, only on a pushed head that is green, and a red run is the worker's CI fix turn first.
- 0009 [A project's own hooks run at kelpie's points](0009-a-projects-own-hooks-run-at-kelpies-points.md): `hooks` runs a project's commands at `publish`, `commit`, `push`, `handover` and nine points for the events Claude Code and Codex offer (`command`, `write`, `after_command`, `turn_start`, `turn_end`, `reset`, `ruling`, `ci`, `done`), for every harness and role, with `worker.guard_hooks` kept as the Claude Code-only layer.

## Decided against

Check this list before proposing an option. Where a later ADR reopened the decision, the line says so.

- adopt Paperclip (0001)
- build on Vibe Kanban (0001)
- a third flock category, or per-project "puppies", inside shep (0001)
- standalone with no shepherd (0001)
- workers as sheep, one fold per project (0002)
- keep kelpie's own shepherd (0003)
- the adopted dog holds the leases (0003, taken later by 0004)
- the dog's sheep named `kelpie`, beside the disabled adopted dog (0003)
- keep `kelpie-dog` (0004)
- every agent session a sheep (0005)
- drop kelpie's process groups (0005)
- keep kelpie's stop ladder and make it complete (0005)
- keep the deep round (0006)
- keep the review loop with a better prompt (0006)
- keep the planning call, off by default (0006)
- a model re-checks each fix (0007, and the deep round's re-check session in 0006)
- keep holding nits back from the worker (0007)
- raise `max_items` instead of freeing a parked item's slot (0007)
- review first and CI after it, as 0006 ordered (0008)
- run CI and the first reviewer at once (0008)
- skip the wait for a local reviewer (0008)
- run a project's checks as Claude Code hooks for every harness (0009)
- let `hooks` replace `worker.guard_hooks` (0009)
- let a hook rewrite the text it judges (0009)
- let a slow hook pass (0009)
- have `publish` cover only the text an agent wrote (0009)
- one hook point for each Claude Code or Codex event (0009)
- run the work item loop's hook points as Claude Code hooks (0009)
