# Dependencies

Whether kelpie merges the bumps bots open (Dependabot), and what it does with one that breaks. #855

## Settings

- `[app.dogs.kelpie.dependencies]`, `#[serde(default)]`, `deny_unknown_fields`: an absent table is `pick = "off"`. `src/settings/dependencies.rs`
- `pick`: `off` (default; every bot's pull request is left alone) or `on`.
- `bots`: logins whose pull requests are bumps, `["dependabot"]` when absent. Compared ignoring case and a `[bot]` suffix. Not the contributions' bot rule: a bump never waits on the maintainer's labels.
- `on_red`: `close` (default) or `leave`. `fix` is not built.
- `web`: paths that make a bump a web bump. A path ending `/` is a folder, any other is a file. Empty means no bump is one.
- `wait_hours` (24): above 0. How long a head's checks may stay pending, or never start, before the bump is red.
- `reread` keeps the running table like the other project tables, so a change waits for the next start. `src/runner/reread.rs`

## Detection

- A bump is an open pull request, not a draft, from a branch of the repo itself, whose author is a listed bot. `is_bump`, `src/runner/dependencies.rs`. A bump takes no slot, no ruling and no model call.
- `ProjectState.bumps` holds `Bump { pull_request, author, branch, head, seen, rebase_asked, e2e, standing, red }`, oldest first. A new head replaces `head` and `seen` and clears `e2e`; `rebase_asked` names the head last asked, so the new head is asked afresh. `src/state/bumps.rs`
- `Runner::follow_dependencies` runs once a `BOARD_POLL` after `follow_contributions`, off the same listing. A bump that left the board is dropped, unless kelpie had decided to close it and still owes its issue. One closed by someone else before kelpie's comment or close skips them and gets the issue. `pick = "off"` forgets all.

## Proving a head

- Behind or conflicting with `origin/main` (`worktree::base_of`): `@<author> rebase` (the login without a `[bot]` suffix, which a mention does not resolve) is posted once per head (`rebase_asked`), as the maintainer's `gh` login, and nothing else is proved on that head. A conflicting bump is asked the same way: Dependabot decides whether it can.
- On `main`: wait `CHECKS_SETTLE` (120 s) from `seen`, as the gate does, then read the checks. `Passed` and `Skipped` are green. Failed is red.
- Pending or absent checks are red once `wait_hours` have passed since `seen`.
- A web bump (`changed_paths` of the head against `origin/main` meets `web`) also needs a passing e2e run. `src/runner/flight/proving.rs`
- The run is one thread at a time: check the head out at `worktrees/bump-<n>`, ask the dog (`E2eRunner`, `lease e2e`'s ask), remove the checkout, send the outcome as news. The dog finds the project from that folder. `src/adapters/e2e_runner.rs`
- Outcomes: exit 0 passes and is saved; any other exit is red and saved with the end of its output; 78 (no host) leaves the bump unmerged and `status` says so, and the dog is not asked again for that head until the runner restarts; 75 (dog unreachable) is asked again at the next poll.
- A proved bump is merged with `Forge::merge` (a merge commit, `--match-head-commit`). Merges come one at a time without a queue: the next bump is behind `main` and is asked to rebase.

## Red

- Under `close`, steps are saved one at a time in `Red.step`, so a restart repeats none: comment naming each failed check (the e2e output tail is not posted), close the pull request (`Forge::close_pull_request`), then open an issue labelled `needs-triage` and `dependencies`. A refused step is tried again at the next poll.
- Under `leave` the reason is logged once and the bump stays open. A new head gives it another chance.
- Out of scope: `on_red = "fix"`.

## Log and status

- One log line each: rebase ask, e2e run (passed, failed), merge, red, close, issue filed. `bump #<n>: ...`
- `status` lists `bumps` with the pull request, author, head and standing, left out when empty.
