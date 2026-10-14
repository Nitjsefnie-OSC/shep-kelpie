# shep-kelpie's home

shep-kelpie keeps everything under `$SHEP_HOME/kelpie`, or the folder `KELPIE_HOME` names:

- `agents`, `builds`, `codex`, `rulings`, `tools`, `totp` and `upgrade`, shared by every project
- `retro-reports/unverified`, with each batch retro the project manager wrote, one file each, which shep-kelpie never reads again
- `github`, with a folder for each owner's App: its id, slug and client id in `app.json`, and its private key in `key.pem`, readable by you alone
- `dog`, with the dog's book and its door, `lease.sock`. The adopted dog gets `SHEP_HOME` and no `KELPIE_HOME` from shep, so it is always under `$SHEP_HOME/kelpie`, even with `KELPIE_HOME` set for your own commands
- `<project>`, with the project's `state.json`, `board.md`, `steps.jsonl`, `usage.jsonl`, worker files, `worktrees` and `builds`, and `pm`, the project manager's folder

So a project can't be named for one of shep-kelpie's own folders, in any case, since a macOS volume reads `Github` as `github`. Socket paths must stay under 104 bytes, so a runner with a long `SHEP_HOME` refuses to start and names the path that is too long. Keep a shepherd's `SHEP_HOME` short.

A runner writes two logs. The shepherd keeps what it prints, one plain line per event such as `#303 dispatched to claude-sonnet-5 (medium)`, in `$SHEP_HOME/logs/<project>-0-out.log`, for you to read. It never holds JSON. The runner appends each step as one JSON object per line, with the time it was logged, to the project's `steps.jsonl`, beside the usage ledger, `usage.jsonl`. `watch` and `usage --import` read that file and not the out log.

A runner or the dog refuses to start while `~/.kelpie` still holds its files from before shep-kelpie's home moved under `$SHEP_HOME`. The message names the folder and says what to do with it: move `projects/<project>`, which holds the project's state, to `$SHEP_HOME/kelpie/<project>`, and delete `wt/<project>`, `targets/<project>`, `shots/<project>` and `playwright/<project>`. Kelpie makes worktrees and builds again, so delete `wt` and `targets` once the project has no open work item: an open work item's state holds the old folder's path, and kelpie cannot move it. Kelpie's state for a project now goes in `$SHEP_HOME/kelpie/<project>/` by default, or under `KELPIE_HOME` when that is set. `shep kelpie doctor` reports a project whose folder is still there, and `shep kelpie add` warns when it registers one. A runner refused this way stays stopped, with the reason in `shep bleats <project>`, until you clear it and run `shep kelpie start <project>`. A runner added before this release restarts instead, until you run `shep kelpie add` again in its checkout. A runner added before this release has no stop code in its flock entry, so it still restarts under shep's backoff until its `stop_exit_codes` is set to `[78]`, which lookout's settings pane can do.
