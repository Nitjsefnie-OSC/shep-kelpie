# Browser

A worker looks at the UI it builds through a browser kelpie runs outside its sandbox. ADR 0011, #858, #859.

## Always

- `[app.dogs.kelpie.browser]` (`enabled` off, `headed` off, `domains` empty) in `src/settings/browser.rs`. `domains` takes the form of `worker.allowed_domains`: a host, or `*.` and a parent. A port, an IP address and `localhost` are refused, since Chromium's resolver rules name hosts and cannot hold one to a port.
- Only a worker's `Work` turn gets it, with a non-empty prompt that is not `COMPACT`, on a project with the table on, on macOS, on a harness whose `drives_a_browser` is true (Claude Code and pi). Reviewers, the project manager, the advisor, the researcher and `attach` get none. `src/runner/browser.rs`, parity row `harness.browser`
- On Linux an enabled table stops the runner at start with `Exit::Refused`, naming #135. `src/sheep.rs`
- `Runner::prepared` opens a `Bridge` (`src/adapters/bridge.rs`) and names its socket in `Reach.browser`. The tools missing is a note and a run without. A bridge that cannot open fails the call, since the instructions promise a browser.
- The socket has a 128-bit random name in the worker folder, checked with `home::socket_fits`. The sandbox may connect to that path only (`fence_the_browser` in `src/adapters/mcp.rs`, called by Claude Code and pi).
- The agent loads it as the MCP server `playwright`, running `kelpie mcp-connect <socket>` (`src/bridge.rs`), which carries stdio to the socket.
- A fresh Playwright MCP server per connection, in its own process group, outside the sandbox: `node <tools>/node_modules/@playwright/mcp/cli.js --config <file> --isolated --output-dir <dir>`, plus `--headless` unless `headed`. `PLAYWRIGHT_BROWSERS_PATH` is `<tools>/browsers`. Dropping the bridge shuts the connection, stops the server's group (TERM, then KILL) and removes the socket.
- The config and output folder are `<project>/browser/<issue>`, made by kelpie and refused if they are a link. The worker cannot write them: the server resolves a file's symlinks, so a folder the worker could write would write anywhere.
- Chromium launches with `--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE <domain>...`, so every other host, every IP address and every WebSocket to an unlisted host fails to resolve.
- Every line is judged both ways (`Filter` in `src/bridge.rs`). Inbound passes `initialize`, `ping`, `tools/list`, notifications, replies, and a `tools/call` on a tool in `TOOLS` whose arguments `browse::judge` allows. Anything else gets a JSON-RPC error with the request's id. Outbound, a `tools/list` reply is cut to `TOOLS` and a line that is not a JSON object is dropped.
- `TOOLS` is an allow-list of 19. A tool a Playwright upgrade adds is refused until listed. Left out: `browser_run_code_unsafe`, `browser_file_upload`, `browser_drop`, `browser_set_storage_state`, `browser_install`.
- `browse::judge` (`src/browse.rs`): a `url` passes only as `http` or `https` to a listed host. Any `filename` is refused, so a screenshot comes back in the tool's reply. `localhost` is not allowed until #861.
- `@playwright/mcp` is pinned exactly in `src/tools/package.json`. `tools install` also fetches Chromium into `<tools>/browsers`. `doctor` says when a project has the browser on and the tools lack it. `src/tools/browser.rs`, `src/doctor/project.rs`
- The worker's instructions name the `browser_*` tools, the hosts, and say to wait in the foreground, since a turn that ends on a background task is never woken under `-p`. `src/runner/instructions.rs`
- The bridge belongs to the turn's flight and ends with it. `carry_calls` refuses a handover while a worker turn holds one, naming the issue. `src/runner/flight/carry.rs`

## Not built

- `localhost`, for a worker's own dev server: #861
- Codex and goose: #863, #864
- Linux: #135
- Shots on the pull request. ADR 0006, 0011

Measurements: `docs/facts/the-browser.md`.
