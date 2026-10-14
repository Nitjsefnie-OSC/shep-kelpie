# Browser

A worker looks at the UI it builds through a browser kelpie runs outside its sandbox. ADR 0011, #858, #859, #860.

## Always

- `[app.dogs.kelpie.browser]` (`enabled` off, `headed` off, `domains` empty) in `src/settings/browser.rs`. `domains` takes the form of `worker.allowed_domains`: a host, or `*.` and a parent. A port, an IP address and `localhost` are refused, since Chromium's resolver rules name hosts and cannot hold one to a port.
- Only a worker's `Work` turn gets it, with a non-empty prompt that is not `COMPACT`, on a project with the table on, on macOS, on a harness whose `drives_a_browser` is true (Claude Code, pi and Codex). Reviewers, the project manager, the advisor, the researcher and `attach` get none. `src/runner/browser.rs`, parity row `harness.browser`
- On Linux an enabled table stops the runner at start with `Exit::Refused`, naming #135. `src/sheep.rs`
- `Runner::prepared` opens a `Bridge` (`src/adapters/bridge.rs`) and names its socket in `Reach.browser`. The tools missing is a note and a run without. A bridge that cannot open fails the call, since the instructions promise a browser.
- The socket has a 128-bit random name in the worker folder, checked with `home::socket_fits`. The sandbox may connect to that path only (`fence_the_browser` in `src/adapters/mcp.rs`, called by Claude Code, pi and Codex).
- The agent loads it as the MCP server `playwright` (Codex through a `-c mcp_servers.playwright` override), running `kelpie mcp-connect <socket>` (`src/bridge.rs`), which carries stdio to the socket.
- A fresh Playwright MCP server per connection, in its own process group, outside the sandbox: `node <tools>/node_modules/@playwright/mcp/cli.js --config <file> --isolated --output-dir <dir>`, plus `--headless` unless `headed`. `PLAYWRIGHT_BROWSERS_PATH` is `<tools>/browsers`. Dropping the bridge shuts the connection, stops the server's group (TERM, then KILL) and removes the socket.
- The config and output folder are `<project>/browser/<issue>`, made by kelpie and refused if they are a link. The worker cannot write them: the server resolves a file's symlinks, so a folder the worker could write would write anywhere.
- Chromium launches with `--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE <domain>...`, so every other host, every IP address and every WebSocket to an unlisted host fails to resolve.
- Every request the browser makes goes through a proxy kelpie runs for it, outside any sandbox, on a loopback port kelpie picks (`src/adapters/bridge/proxy.rs`, opened and dropped with the `Bridge`). The resolver rules name hosts and cannot hold a host to a port, so once the browser may open `localhost` (#861), page code the worker wrote could reach every local service from outside the sandbox. `docs/facts/the-second-harness.md` measured the same reach for `allowLocalBinding`.
- Chromium launches with `--proxy-server=http://127.0.0.1:<port>` and `--proxy-bypass-list=<-loopback>`, so loopback goes through the proxy too. The `Bridge` writes the config (`config.json` in the call's browser folder) that carries them with the resolver rules, which `EXCLUDE 127.0.0.1` so the browser can reach the proxy. From Chromium's documentation, not yet measured here.
- The proxy allows a `CONNECT` tunnel, or an absolute `http` URL, only to an `Origins` entry: each `browser.domains` host (`*.` and a parent included) on ports 80 and 443. #861 adds the dev server's one port. Anything else gets a 403 and no connection is made.
- A listed host that resolves to an address that is not globally routable is refused: loopback, private, link-local, shared (100.64.0.0/10), benchmarking (198.18.0.0/15), documentation, multicast and reserved IPv4 ranges, and all IPv6 outside global unicast (2000::/3) less its special-purpose blocks, which takes in NAT64 and 6to4. An IPv4-mapped IPv6 address is read as its IPv4. One such address among several refuses the host. The address that was checked is the one dialed, so a second lookup cannot answer differently.
- A request is read as the forwarder reads one (`src/adapters/forwarder/http.rs`): the head is capped at 64 KiB, a body at 64 MiB, the whole request must arrive in 30 seconds, and 64 connections are served at once, the rest getting a 503. A relayed request is sent on in its own form with `Connection: close`, the `Host` of its URL, and the proxy-hop headers dropped. A tunnel has no idle limit and ends with the bridge.
- A refusal is one log line on the runner's stderr, `browser proxy refused <host>:<port>: <why>`, never the path or query.
- The bridge's `Filter` and `browse::judge` are unchanged, and the resolver rules stay as a second layer.
- Every line is judged both ways (`Filter` in `src/bridge.rs`). Inbound passes `initialize`, `ping`, `tools/list`, notifications, replies, and a `tools/call` on a tool in `TOOLS` whose arguments `browse::judge` allows. Anything else gets a JSON-RPC error with the request's id. Outbound, a `tools/list` reply is cut to `TOOLS` and a line that is not a JSON object is dropped.
- `TOOLS` is an allow-list of 19. A tool a Playwright upgrade adds is refused until listed. Left out: `browser_run_code_unsafe`, `browser_file_upload`, `browser_drop`, `browser_set_storage_state`, `browser_install`.
- `browse::judge` (`src/browse.rs`): a `url` passes only as `http` or `https` to a listed host. Any `filename` is refused, so a screenshot comes back in the tool's reply. `localhost` is not allowed until #861.
- `@playwright/mcp` is pinned exactly in `src/tools/package.json`. `tools install` also fetches Chromium into `<tools>/browsers`. `doctor` says when a project has the browser on and the tools lack it. `src/tools/browser.rs`, `src/doctor/project.rs`
- The worker's instructions name the `browser_*` tools, the hosts, and say to wait in the foreground, since a turn that ends on a background task is never woken under `-p`. `src/runner/instructions.rs`
- The bridge belongs to the turn's flight and ends with it. `carry_calls` refuses a handover while a worker turn holds one, naming the issue. `src/runner/flight/carry.rs`

## Not built

- `localhost`, for a worker's own dev server, which adds its one port to the proxy's `Origins`: #861
- goose: #864
- Linux: #135
- Shots on the pull request. ADR 0006, 0011

Measurements: `docs/facts/the-browser.md`.
