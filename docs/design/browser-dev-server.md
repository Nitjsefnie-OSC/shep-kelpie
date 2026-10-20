# Browser dev server

A worker's sandbox binds no port and a headless turn has no background tasks, so kelpie starts the project's dev server for it. `runner/dev_server.rs`, `crates/core/src/ports/dev_server.rs`, `crates/adapters/src/adapters/dev_server.rs`

- It starts before each worker turn that has the browser, from `.claude/launch.json` as `origin/main` has it (`git show`, never the worktree, so a worker cannot widen its own sandbox). `browser.configuration` names one, else the first. Fields read: `runtimeExecutable`, `runtimeArgs`, `port`, `cwd`, `autoPort`. No file on `main` starts nothing. `runner/dev_server/launch.rs`
- `cwd` resolves under the worktree. One that leaves it by `..`, an absolute path or a link is refused, and the instructions say why.
- Its own `srt` `Policy`: writes the worktree and build folder under the fence's `no_write` and `no_read`, reaches `browser.domains`, `listen: true`, `services: ["com.apple.FSEvents"]`, the keychain blocked as the call's is, and `NODE_USE_ENV_PROXY=1`.
- Port: the configured one when free and not planned for another work item, whose server may not have bound it yet (port 0 is no port: `autoPort` replaces it, else the turn runs without a server). Under `autoPort`, kelpie picks a free port, sets `PORT` and replaces the configured number where it is a whole `runtimeArgs` entry. Otherwise a taken port runs the turn without a dev server, and the instructions say so.
- A port taken between the plan and the start refuses the start. Ready when it answers on its port while the server kelpie started still runs, 60 seconds at most. Only then does the browser's `DevPort` gate open that port (filter and proxy both read it), and it shuts when the turn's server is dropped. A server that has not answered leaves the port shut. If it never answers the turn still runs, and the instructions name `dev-server.log` in the build folder.
- It is a child of the runner in its own process group, stopped (TERM, then KILL) when its `Started` guard drops, whatever ended the turn (ADR 0005).
- The plan is made under the runner lock and lasts until the turn ends, so a retry reuses its port. The start and wait run on the call thread, off the lock.
- A resumed session does not re-read its instructions, so news that the server is down goes in that call's prompt. A new session gets it in the instructions, where a standing line points to the log.

The browser it serves: `docs/design/browser.md`. Measurements: `docs/facts/the-browser.md`.
