# Settings

A project's settings are its `[app.dogs.kelpie]` table. `shep kelpie get [<key>]` prints it, or one dotted key, as TOML, and `shep kelpie set <key> <value>` changes one key, such as `shep kelpie set git.merging auto` or `shep kelpie set agents.reviewers '["qwen"]'`. The value is a TOML value, and text that is not one is a string. `set` writes only a table the runner would accept, and refuses to move `git.checkout` or `git.remote` onto a checkout or repo another project runs, as `add` does. It says when a running runner takes the change: at its next wake, or for `git.checkout`, `git.remote` and `hooks`, when it next starts. Lookout edits the table too, in the runner's pane. Every setting sits in a table of its own.

Two example files hold what every setting means, in their comments, and nothing else says it:

- [`settings.example.toml`](https://github.com/shep-pm/shep-kelpie/blob/main/settings.example.toml) has every project key, and shows a runner's Flockfile entry for a project set up by hand
- [`kelpie-settings.example.toml`](https://github.com/shep-pm/shep-kelpie/blob/main/kelpie-settings.example.toml) has shep-kelpie's own settings, shared by every project: the `[kelpie]` section of `dogs.toml`, which lookout edits in the dog's pane. Keep `dogs.toml` private, since the webhook's URL is a credential

A change reaches a running runner at its next wake, within a minute when idle. `git.checkout`, `git.remote`, `hooks` and kelpie's gateways wait for its next start, and the log says so. The dog reads `[kelpie.leases]`, and each review bot's window from its agent file, only when it starts, so after a change to either run `shep restart kelpie`.

A key kelpie no longer reads, such as `review.reviewers`, `coderabbit` or `planning`, stops the runner, naming the key and what replaces it. So does each top-level key from before the tables: `repo`, `forge`, `merge_authority`, `ci`, `max_items`, `max_parked` and `skills.ci` name the key that took each over, and `private_names` says to check text for your own words with a hook in `hooks` instead, which kelpie runs for every harness and role.

## What `add` writes

`add` writes the project's settings with these values. What each setting does is in [`settings.example.toml`](https://github.com/shep-pm/shep-kelpie/blob/main/settings.example.toml).

- `git.checkout` and `git.remote`: the checkout and the GitHub repo its `origin` names
- `git.merging = "ask"` and `git.issues = "ask"`
- `ci.block` is on when the checkout has `.github/workflows`, and `ci.fix_attempts = -1`
- `concurrency.active_items = 1` and `concurrency.pending_rulings = 2`
- `budget.per_item_usd = 10`
- `agents.implementers = ["sonnet-high"]`
- `agents.reviewers = ["defect-hunter"]`, with `qwen` first when `~/.claude/scripts/qwen-review.sh` exists, and no review bot
- `contributions.pick = "off"`, `contributions.security_reviewer = "security-reviewer"`, `contributions.quiet_days = 7` and `contributions.nudge_days = 3`
- `pacing.enabled = true`
- `worker.allowed_domains = []`, `worker.turn_timeout = 60` and `worker.compact_window = 200000`

The checkout is the project's repo, and shep-kelpie runs `git fetch`, `git worktree` and `git branch` against its `.git`. Its worktrees, build folders and state go under `$SHEP_HOME/kelpie/<project>`, never inside it. Add from a clone you don't work in if you'd rather keep your own checkout out of it.

## Browser

A project can let its worker look at the UI it builds. Kelpie runs a browser outside the worker's sandbox, where Chromium cannot start, and the worker reaches it over a socket kelpie filters. It is off unless the project turns it on:

```toml
[app.dogs.kelpie.browser]
enabled = true
headed = false
domains = ["pokemon-go-api.github.io", "*.leekduck.com"]
```

- `shep kelpie tools install` installs the Playwright MCP server and Chromium under `<kelpie home>/tools`, and a project that turns the browser on without them gets a MISSING `browser` line in `doctor`.
- The worker's working turns on Claude Code and pi have it, as the `playwright` server. A reviewer, the project manager, the advisor, the researcher and `attach` do not, and Codex and goose do not yet.
- Chromium resolves only `domains`. Every other host and every IP address fails to resolve, and the filter refuses a URL that is not `http` or `https` to one of them. `localhost` is not allowed yet. A tool call that names a `filename` is refused, and a screenshot comes back in the tool's reply.
- `headed = true` opens a window you can watch.
- macOS only. On Linux an enabled table stops the runner when it starts, naming shep-pm/shep-kelpie#135.
- A handover waits while a worker's turn holds a browser, since the browser lives in the running image.
