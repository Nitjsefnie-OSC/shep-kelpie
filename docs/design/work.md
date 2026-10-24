# Work

Projects, slots, issue writer. The board and work items are in `board.md`, the project manager in `pm.md`.

## Projects

- `shep kelpie add`: registers stopped runner + `[app.dogs.kelpie]` table; takes `--base`, writes `git.base`; reads all, then makes only what's missing. `src/flock/verbs.rs`
- `add` refuses a checkout or repo another project's table names, even a broken one (two runners, same issues). `src/flock/verbs.rs`
- Project runs exactly while its runner's sheep runs; no paused state. #268
- `finish`: open items end, nothing new picked, then stops the sheep; `finishing` saved so restart keeps finishing. `add`, `adopt`, `rework` refused meanwhile; flag cleared just before stop. #303
- `get [<key>]` prints the project's table or one dotted key; `set <key> <value>` reads the value as TOML (else a string), checks the whole result as a runner start and reread do (settings, agents, reviewers, rulings, gateway shapes with no key read, skills; and for a moved `git.checkout` or `git.remote`, the checkout and `add`'s one-runner-per-checkout-and-repo rule), and only then writes the table back; refusals write nothing. A table written another way (lookout, `shep copy`) gets the same checks at runner start (refuses, exit 78) and reread, and a refusal alerts. Says whether a running runner takes the change at its next wake or start. Not `[kelpie]`. #273, #965, `src/flock/set/mod.rs`
- `start`, `pause`, `finish` touch only sheep kelpie started as `runner <project>` (never start one the maintainer stopped); `start` waits 30s for JSON `status`. `src/flock/verbs.rs`
- `pause`: drain, then wait out calls and any merge (up to 1h), then stop sheep (stop mid-merge can leave a merged PR's item open); failed wait sends `undrain`. A Claude Code or pi worker turn ends at its next tool call (workers.md); `pause` names those items and waits 10 min for it (a full test run is ~6 min), then stops the call as `shep stop` does. Other calls wait ceiling + 5 min. #268, #821

## Slots

- `concurrency.active_items` (1): a project's slots, each item own branch + worktree (a parked PR must not hold the project). #103, `runner/slots.rs`
- Slot bounds model calls only: parked or `Done` item frees it, model-free phases slotless; one needing a model again goes before new work. A call on a followed contribution takes one too, before new work (contribution-security.md). #355, #621, #1162, ADR 0007
- `concurrency.pending_rulings` (2): no new work while that many items parked (alerts can't open PR after PR); `add` ignores it. #355
- Issue naming a file a parked branch changes skipped as `overlap` (ruling may still change it). #355
- Resuming a parked session after 1h repays cache write; accepted. shep-pm/shep#707
- Bot window grant serves one item, never inherited (would summon twice in a window the dog counted once). #103

## Not built

- Overlapping items bundled when small, chained otherwise; today overlap shows as a conflict turn. `docs/design-log.md` Decided, not built
- Oversized work split into chained items by the worker; today issue writer scopes first. ADR 0006
