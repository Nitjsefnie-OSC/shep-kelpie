# Architecture decision records

Each ADR below is one decision that is hard to reverse. A later ADR can reopen part of an earlier one, and says so in its own text.

## Decisions

- 0001 [Kelpie stands on shep, not in it](0001-kelpie-stands-on-shep-not-in-it.md): kelpie is its own repo built on the published shep crates, and a generic gap becomes a shep issue.
- 0002 [Projects are sheep, workers are lambs](0002-projects-are-sheep-workers-are-lambs.md): each project runs as one long-lived sheep and its workers are that sheep's lambs.
- 0003 [Runners live in the maintainer's own shepherd](0003-runners-live-in-the-maintainer-s-own-shepherd.md): runners and the dog are sheep of the maintainer's own shepherd, and kelpie is adopted as a dog.
- 0004 [The adopted kelpie is the lease dog](0004-the-adopted-kelpie-is-the-lease-dog.md): the adopted `kelpie` holds the leases through the shepherd channel shep 0.12 gives it, and `kelpie-dog` is gone.
- 0005 [shep stops every lamb](0005-shep-stops-every-lamb.md): shep's stop reaches every lamb in the tree, and kelpie drops its own stop ladder, orphan sweep and pid files.
- 0006 [One agent builds, reviewers run once in series](0006-one-agent-builds-reviewers-run-once.md): the issue says what done means, one agent builds it, and each listed reviewer reads once with one fix turn, with no loop and no judge. 0011 reopened the parked shots and preview half.
- 0007 [Review rules for a live pull request](0007-review-rules-for-a-live-pull-request.md): nits and a merge ruling's `no` get a fix turn, a late bot is read, and a parked item frees its slot.
- 0008 [No reviewer reads a head until CI has passed on it](0008-no-reviewer-reads-a-head-until-ci-has-passed-on-it.md): a reviewer starts, or a bot is summoned, only on a pushed head that is green, and a red run is the worker's CI fix turn first.
- 0009 [A project's own hooks run at kelpie's points](0009-a-projects-own-hooks-run-at-kelpies-points.md): `hooks` runs a project's commands at `publish`, `commit`, `push`, `handover` and nine points for the events Claude Code and Codex offer (`command`, `write`, `after_command`, `turn_start`, `turn_end`, `reset`, `ruling`, `ci`, `done`), for every harness and role, with `worker.guard_hooks` kept as the Claude Code-only layer.
- 0010 [Kelpie finishes a contribution on its contributor's branch](0010-kelpie-finishes-a-contribution-on-its-contributors-branch.md): a quiet contributor is asked, a yes or silence is a release, and kelpie then pushes to their branch, never a second pull request.
- 0011 [A worker sees the UI through a browser outside its sandbox](0011-a-worker-sees-the-ui-through-a-browser-outside-its-sandbox.md): a worker's `Work` turn on Claude Code or pi drives a Playwright MCP server that kelpie runs outside the sandbox, behind a socket that passes only an allow-list of tools and listed domains.
- 0012 [Kelpie's numbers go out on the shepherd channel](0012-kelpie-s-numbers-go-out-on-the-shepherd-channel.md): the runner and the dog send `kelpie_*` series as channel samples to shep's metrics dog, with bounded labels, and detail by work item stays in the ledger.
- 0013 [Runners tell the dog in notes, not metric samples](0013-runners-tell-the-dog-in-notes-not-metric-samples.md): a runner's lease and window totals go up as shep notes, which the metrics dog never reads, and the dog reads an older runner's samples for one release.
- 0014 [Tickets come from a source, and the board stays in kelpie](0014-tickets-come-from-a-source-the-board-stays-in-kelpie.md): each project names one ticket source, GitHub issues by default or a local store, read and written through a `Tickets` port by opaque id, while the order, holds, picks and overlap check stay kelpie's.

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
- a second pull request from a new branch, for a contribution (0010)
- adopt a quiet contribution without asking its contributor (0010)
- wait for a contributor's explicit yes before adopting (0010)
- Chromium inside `srt` (0011)
- Claude Code starting the Playwright MCP server unsandboxed (0011)
- the e2e host alone for a look at the UI (0011)
- raw CDP from a crate in place of Playwright (0011)
- shots on the pull request, in place of a live look (0011, and parked in 0006)
- keep lease totals as metric samples and have shep's metrics dog skip kelpie's names (0013)
- rename lease totals to names the metrics dog takes (0013)
- a socket of kelpie's own between runner and dog, for lease totals (0013)
- kelpie owns the tickets and GitHub or ClickUp sync into them (0014)
- a `shep-kanban` dog holding the local ticket store, for now (0014)
- more than one ticket source per project (0014)

### Decided against without an ADR

Worker retros of 10 to 12 October 2026 kept proposing these. The maintainer turned them down on 12 October.

- let the worker's sandbox write `packed-refs` or the shared `.git`: denied on purpose, and git is told not to try instead
- allow `/tmp` in the worker's fence: every worker shares it. Tests honour `TMPDIR`, and the worker's instructions now send its scratch files there (#951)
- run `lease run cargo-test` outside the sandbox: it hands a worker an unsandboxed exec path
- let workers run `gh api` or read-only GraphQL: a worker's `gh` is the maintainer's login
- point `TMPDIR` under the build folder so socket tests bind: the bind is denied, not the path
- mark tests the sandbox cannot run `#[ignore]`: CI never runs ignored tests, so coverage is lost
- drop review findings in files outside the branch's diff: real findings sit there
- forbid `std::fs` in `src/runner` with clippy: kelpie's own folders use it legitimately
- a `just` recipe or test script beside the documented cargo commands: a second command shape
- `cargo deny` or `cargo tree -d` for duplicate crates: upkeep for one incident
- file a worker's own out-of-scope findings as issues in its words (13 October): worker-written text would reach a public issue unchecked. A worker's own find goes in its final message, where the project manager's batch retro reads it
- merge `origin/main` at the start of every round: kelpie merges main, and an early merge adds conflicts
