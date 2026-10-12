# Work

Projects, the board, work items, slots, issue writer, project manager.

## Projects

- `shep kelpie add`: registers stopped runner + `[app.dogs.kelpie]` table; refuses default branch not `main`; reads all, then makes only what's missing. `src/flock/verbs.rs`
- `add` refuses a checkout or repo another project's table names, even a broken one (two runners, same issues). `src/flock/verbs.rs`
- Project runs exactly while its runner's sheep runs; no paused state. #268
- `finish`: open items end, nothing new picked, then stops the sheep; `finishing` saved so restart keeps finishing. `add`, `adopt`, `rework` refused meanwhile; flag cleared just before stop. #303
- `start`, `pause`, `finish` touch only sheep kelpie started as `runner <project>` (never start one the maintainer stopped); `start` waits 30s for JSON `status`. `src/flock/verbs.rs`
- `pause`: drain, wait out calls (ceiling + 5 min) and any merge (up to 1h), then stop sheep (stop mid-merge can leave a merged PR's item open); failed wait sends `undrain`. #268

## Board and items

- Tracker: GitHub Issues + `ready-for-agent`; board of work items lives in kelpie (maintainer triages where they already work). `src/board.rs`
- Issue writer: `role: issue-writer`, Claude Code only, Opus 5.5 at medium. Scopes or splits each request to one PR, labels `agent:<name>`; body asks for self-contained pieces + acceptance criteria, never mentions cost. No planning call (plans no better, shared plans spread bugs). ADR 0006, #324, `agents/issue-writer.md`
- `shep kelpie issue "<request>"`: headless, fresh `origin/main`, files `ready-for-human`; fails on issue missing criteria or labels; no lamb, unpaced (command waits on it). `src/issues.rs`
- `--interactive`: `claude` with no `allow` rule (maintainer approves each command outsider issue text could steer), files `ready-for-agent`. `src/issues.rs`
- Issue-writer guard: allow-list of `gh issue` reads, creates, label/link edits on issues it filed; no shell syntax, no git (git options too wide for deny-list). `src/guard/issues.rs`
- PostToolUse ledger records filed issues so edits/links reach only those; fails closed; hook seeing `tool_response.stdout` unchecked. `src/guard/issues.rs`
- Design settled first: interactive runs vendored `grill-with-docs`, headless writes `## Open questions` with recommended answers; facts from repo, never maintainer. #365
- Interactive docs land on `kelpie/docs-<id>` worktree, writes confined to `GLOSSARY.md` + `docs/adr`, then draft PR with no `Resolves` (checkout untouched, no runner takes it). #365
- Unlabelled ready issue + several implementers: runner makes pick-only issue-writer call and labels it itself; failure raises `stuck` `unlabelled`. #381, `src/runner/flight/label.rs`
- Board order: `priority: P0` to `P3`, then unlabelled, oldest first; adoptions and reworks ahead; blocked or skipped issue never holds up next. Keeps an `order`: new or re-prioritised issue goes after last of its priority; labels bump, move verbs fine-tune. #144, #494, #495, `src/board.rs`
- `agent:<name>` label picks implementer, `!` pins against fallback; unknown or doubled label skipped; kelpie only adds missing `agent:` labels (deleting strips every issue). #381
- Issue with sub-issues never worked (they are, instead); closed when all sub-issues closed. `src/board.rs`
- Held issue carries `in-progress` (courtesy; refusal only logged). #199
- Merged issues never retaken until they read closed (`Resolves` close lags merge); `drop` keeps the PR, lists issue `dropped` until it leaves the ready list. #172, #481
- Kelpie works its own repo from a separate clone, never the session checkout; worker rules in `CLAUDE.md`; merged changes reach runner only on upgrade. #140

## Slots

- `concurrency.active_items` (1): a project's slots, each item own branch + worktree (a parked PR must not hold the whole project). #103, `src/runner/slots.rs`
- Slot bounds model calls only: parked item frees it, model-free phases run slotless; item needing a model again goes before new work. #355, ADR 0007
- `concurrency.pending_rulings` (2): no new work while that many items parked (alerts can't open PR after PR); `add` ignores it. #355
- Issue naming a file a parked branch changes skipped as `overlap` (ruling may still change it). #355
- Resuming a parked session after 1h repays cache write; accepted. shep-pm/shep#707
- Bot window grant serves one item, never inherited (would summon twice in a window the dog counted once). #103

## Project manager

- Runner writes `board.md` so PM never runs git or gh (code-written board decides as well). Keeps newest 50 events + PM cursor; issue bodies quoted (600 chars) behind `> `; text naming this machine withheld. #326, `src/runner/briefing.rs`
- Worker silent 10 min reads as idle (PM told only "busy" misses a hung worker). #326
- Overlap from `git diff` + pairwise `git merge-tree` (git 2.38+), cached by commit, run without runner lock (trigger never waits on git). #326
- `agents.pm` opts a project in (`pm`, Opus 5.5 at medium; Sonnet cost about the same here); without one board rule picks and every ruling waits on maintainer. `src/runner/pm.rs`, #327
- Code wakes it, never a timer: free slot with 2+ choices, new conflict, stuck item, worker question, `shep kelpie tell`. #327
- Reads project worktrees + each item's diff (messages alone miss defects); secret-shaped files and kelpie secrets stay denied. #413, `src/runner/pm/call.rs`
- No git or gh: kelpie writes diffs (64 KiB each) + a `still-red` item's CI log. #452
- Tools `Read,Glob,Grep,Edit,Write`; writes only to append-only `pm-notes.md` via `kelpie confine --append`. #327
- Answer is one JSON object (`pick`, `hold`, `unstick`, `answer`, `follow_up`, `reply`) checked against the board; anything else dropped + logged. #327
- Pick of none lasts 30 min with nothing open; hold ends when an item closes. #327
- One session resumed per wake, compacted past 100k; failed call passed over 10 min, board rule picks meanwhile. #327
- Answers or escalates worker questions, holding the ruling from maintainer one wake (10 min max). #437
- Decides `follow-up` rulings, filing `ready-for-agent` or `needs-triage`. #510, #511
- `shep kelpie pm` hands its session to maintainer's terminal like `attach`; no wake runs under them. #327

## Not built

- Overlapping items bundled when small, chained otherwise; today overlap shows as a conflict turn. `docs/design-log.md` Decided, not built
- Oversized work split into chained items by the worker; today issue writer scopes first. ADR 0006
