# A worker sees the UI through a browser outside its sandbox

ADR 0006 removed shots and the preview as parked, not designed out. This reopens that half: a worker can look at the UI it builds, through a browser kelpie runs outside the worker's sandbox.

What changed since 0006:

- Kelpie runs the maintainer's own projects now, and shep's queue holds web issues.
- The web dependency bumps of #855 change what a page looks like without a test saying so.
- Chromium cannot start under `srt` 0.0.77. Its profile allows no `mach-register`, and headless Chromium dies with `MachPortRendezvousServer: Permission denied (1100)` (`docs/facts/the-browser.md`). So the browser cannot sit inside the fence the way the code graph server does.

The decision: with `[app.dogs.kelpie.browser] enabled`, a worker's `Work` turn on Claude Code or pi gets a `playwright` MCP server. The agent runs `kelpie mcp-connect <socket>`, the socket belongs to a bridge in the runner, and the bridge starts the Playwright MCP server outside the sandbox, one fresh server per connection. Kelpie reads every message both ways. Only an allow-list of tools passes, a URL passes only as `http` or `https` to a listed domain, and no `filename` passes. Chromium resolves only the listed domains. Reviewers, the project manager, the advisor and the researcher get no browser. The design is in `docs/design/browser.md`.

## Considered options

- **Chromium inside `srt`.** It cannot start (above), and `srt` cannot add the Mach service.
- **Claude Code starting the Playwright MCP server itself, unsandboxed.** Every call runs whole inside the sandbox since #128, hooks and MCP servers included, so the server would be inside it and fail the same way.
- **The e2e host alone.** It gives no interactive look, and it needs a machine the maintainer can lose.
- **Raw CDP from a crate.** Its network-idle event never fired with Vite open (30 s timeout), where Playwright's `networkidle` settled in about 1.1 s. Playwright also brings the page snapshot the agent reads.
- **Shots on the pull request.** They stay parked: the worker needs the look while it builds, not a picture after.

## Consequences

- A socket the worker can connect to reaches a browser the worker cannot read. Anything in the sandbox can write to that socket, so the bridge treats every line as hostile and not only the agent's.
- macOS only. On Linux an enabled table stops the runner at start, naming #135, which is where `srt` could run Chromium.
- The bridge lives in the runner's memory, so a handover waits while a worker turn holds a browser.
- Codex and goose get none until #863 and #864. `docs/parity.toml` has the row.
- `localhost` is not on the list until #861, so a worker sees a deployed page and not its own dev server.
