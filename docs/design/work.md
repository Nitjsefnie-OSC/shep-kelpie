# Work

Projects, the board, work items, slots, issue writer. The project manager is in `pm.md`.

## Projects

- `shep kelpie add`: registers stopped runner + `[app.dogs.kelpie]` table; refuses default branch not `main`; reads all, then makes only what's missing. `src/flock/verbs.rs`
- `add` refuses a checkout or repo another project's table names, even a broken one (two runners, same issues). `src/flock/verbs.rs`
- Project runs exactly while its runner's sheep runs; no paused state. #268
- `finish`: open items end, nothing new picked, then stops the sheep; `finishing` saved so restart keeps finishing. `add`, `adopt`, `rework` refused meanwhile; flag cleared just before stop. #303
- `get [<key>]` prints the project's table or one dotted key; `set <key> <value>` reads the value as TOML (else a string), checks the whole result as a runner start and reread do (settings, agents, reviewers, rulings, gateway shapes with no key read, skills; and for a moved `git.checkout` or `git.remote`, the checkout and `add`'s one-runner-per-checkout-and-repo rule), and only then writes the table back; refusals write nothing. A table written another way (lookout, `shep copy`) gets the same checks at runner start (refuses, exit 78) and reread, and a refusal alerts. Says whether a running runner takes the change at its next wake or start. Not `[kelpie]`. #273, #965, `src/flock/set.rs`
- `start`, `pause`, `finish` touch only sheep kelpie started as `runner <project>` (never start one the maintainer stopped); `start` waits 30s for JSON `status`. `src/flock/verbs.rs`
- `pause`: drain, then wait out calls and any merge (up to 1h), then stop sheep (stop mid-merge can leave a merged PR's item open); failed wait sends `undrain`. A Claude Code or pi worker turn ends at its next tool call (workers.md); `pause` names those items and waits 10 min for it (a full test run is ~6 min), then stops the call as `shep stop` does. Other calls wait ceiling + 5 min. #268, #821

## Board and items

- Tracker: GitHub Issues + `ready-for-agent`; board of work items lives in kelpie (maintainer triages where they already work). `src/board.rs`
- A ticket's id is a `TicketId` (`src/ticket.rs`): a GitHub issue number, or `k12` for a local ticket (ADR 0014). The board, dispatch, the briefing and every `Tickets` port method name tickets; triage, adoption and the label flight keep issue numbers and turn each into a ticket at the port, since they handle only GitHub issues. The state file is still keyed by issue numbers, and #1010 moves it to ticket ids. #1005, `src/ticket.rs`, `src/board.rs`, `src/runner/dispatch.rs`
- Issue writer: `role: issue-writer`, Claude Code only, Opus 5.5 at medium. Splits each request into the smallest pieces that ship alone, labels `agent:<name>` and priority; each piece has `Touches:`, concrete files, named tests, status label after its links, never mentions cost. No planning call (plans no better, shared plans spread bugs). Claude Code starts it on `--tools Bash,Read,Glob,Grep,Skill` (its commands are its guard's, it reads with Read/Glob/Grep, `Skill` for what the repo's plugins carry) and no `--strict-mcp-config`, so the servers it loads still load; `ISSUES_DENY` stays as the second fence. #985, `docs/facts/headless-claude-code.md`. ADR 0006, #324, `agents/issue-writer.md`
- `shep kelpie issue "<request>"`: headless, fresh `origin/main`, files `ready-for-human`; fails on issue missing criteria or labels; no lamb, unpaced (command waits on it). `src/issues.rs`
- `--interactive`: `claude` with no `allow` rule (maintainer approves each command issue text could steer), files `ready-for-agent`. Previews each issue's `agent:` label and asks to apply them, none or some; no label = default implementer, so its guard allows zero or one. No `--tools` either: that session writes `GLOSSARY.md` and `docs/adr`, and the maintainer's own settings stay in force beside it. `src/issues.rs`, `src/issues/prompt.rs`
- Issue-writer guard: allow-list of `gh issue` reads, creates, label/link edits on issues it filed; no shell syntax, no git (git options too wide for deny-list). `src/guard/issues.rs`
- PostToolUse ledger records filed issues so edits/links reach only those; fails closed; hook seeing `tool_response.stdout` unchecked. `src/guard/issues.rs`
- Design settled first: interactive runs vendored `grill-with-docs`, headless writes `## Open questions` with recommended answers; facts from repo, never maintainer. #365
- Interactive docs land on `kelpie/docs-<id>` worktree, writes confined to `GLOSSARY.md` + `docs/adr`, then draft PR with no `Resolves` (checkout untouched, no runner takes it). #365
- Unlabelled ready issue + several implementers: runner makes pick-only issue-writer call and labels it itself; failure raises `stuck` `unlabelled`. #381, `src/runner/flight/label.rs`
- Board order: `priority: P0` to `P4`, then unlabelled, oldest first; adoptions and reworks ahead; blocked or skipped issue never holds up next. Keeps an `order`: new or re-prioritised issue goes after last of its priority; `board` lists ready issues numbered, `board move <issue> <position>` and `board top <issue>` reorder them; a non-ready issue or position past the end is refused. #144, #494, #495, `src/board.rs`
- Adopt: `adoptable` refuses a branch a worktree kelpie did not make holds (`git worktree list --porcelain`), naming it, so `adopt` queues nothing that cannot start. A queued PR whose pass fails with `AdoptError::Worktree` raises one `stuck` `adopt-failed` ruling (id is the PR number, no work item), raised again only when the error changes (an answered one included, until a restart), a failure to list the worktrees raises none, cleared when a pass adopts it or it leaves the queue. While any queued PR fails, `status` lists it under `skipped` (`reason: adopt`) with `error` and `since` (in memory, reset on restart). #713, `src/runner/adopt.rs`
- `agent:<name>` label picks implementer, `!` pins against fallback; unknown or doubled label skipped; kelpie only adds missing `agent:` labels (deleting strips every issue). #381
- Issue with sub-issues never worked (they are, instead); closed when all sub-issues closed. The sweep also closes an open issue off the ready list once its sub-issues are all closed with one completed (all not planned stays open); a sub-issue still open on the board's reread stops the close, and one with none completed is read again when its count changes or an hour on. #738, `src/runner/parent.rs`
- Held issue carries `in-progress` (courtesy; refusal only logged). #199
- Merged issues never retaken until they read closed (`Resolves` close lags merge); `drop` keeps the PR, lists issue `dropped` until it leaves the ready list. #172, #481
- Kelpie works its own repo from a separate clone, never the session checkout; worker rules in `CLAUDE.md`; merged changes reach runner only on upgrade. #140

## Slots

- `concurrency.active_items` (1): a project's slots, each item own branch + worktree (a parked PR must not hold the whole project). #103, `src/runner/slots.rs`
- Slot bounds model calls only: parked item frees it, model-free phases run slotless; item needing a model again goes before new work. A call on a followed contribution takes a slot too, ahead of new work (`contribution-security.md`). #355, #621, ADR 0007
- `concurrency.pending_rulings` (2): no new work while that many items parked (alerts can't open PR after PR); `add` ignores it. #355
- Issue naming a file a parked branch changes skipped as `overlap` (ruling may still change it). #355
- Resuming a parked session after 1h repays cache write; accepted. shep-pm/shep#707
- Bot window grant serves one item, never inherited (would summon twice in a window the dog counted once). #103

## Not built

- Overlapping items bundled when small, chained otherwise; today overlap shows as a conflict turn. `docs/design-log.md` Decided, not built
- Oversized work split into chained items by the worker; today issue writer scopes first. ADR 0006
