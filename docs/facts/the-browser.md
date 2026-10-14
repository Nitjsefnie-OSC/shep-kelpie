# The browser

What was measured behind a worker's browser outside its sandbox (ADR 0011, shep-pm/shep-kelpie#858). Most entries were recovered from the removed bridge's reviews and the experiments repo, in git history at `69e0f710^`. Where the recovered notes name no date, the entry says so.

- Headless Chromium cannot start under `srt` 0.0.77 (2026-09-30, macOS). Its profile allows no `mach-register`, and Chromium dies with `bootstrap_check_in ... MachPortRendezvousServer: Permission denied (1100)`. So the browser runs outside the sandbox.
- Chromium's `--host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE <host>"` also blocks IP literals and WebSockets to a host it maps away (2026-09-29, Chromium 154). The rules name hosts and cannot hold a host to a port, so a proxy holds the ports (#860) and the rules stay as a second layer.
- The Playwright MCP server's own `file:` block does not hold: `view-source:file://` read any file past it. Kelpie's filter passes a URL only as `http` or `https` to a listed host. No date recorded.
- The server's `--allowed-origins` is not a security boundary, so kelpie does not rely on it. No date recorded.
- The server's `checkFile` (Playwright 1.64) resolves symlinks, so its `--output-dir` must be a folder the worker cannot write. No date recorded.
- A filename check followed by a later write let a swapped folder write outside the worktree: a review raced a folder against a link. So every `filename` is refused, and a screenshot comes back in the tool's reply. No date recorded.
- A worker that ends a turn on a background task is never woken under `claude -p`. So the instructions say to wait in the foreground. No date recorded.
- A socket listed in `allowUnixSockets` connects even from inside an unreadable folder. A leftover process cannot reach a later call's socket, since each call's name is a new 128-bit random one. No date recorded.
- Raw CDP's network-idle event never fired with Vite open (30 s timeout), while Playwright's `networkidle` settled in about 1.1 s. No date recorded.
- `kelpie tools install` took 11 s with a warm npm cache, with `@playwright/mcp` and Chromium. No date recorded, and a cold cache takes longer.
- Not yet measured here, from Chromium's documentation (#860): `--proxy-server=http://127.0.0.1:<port>` sends the browser's requests to the proxy, and `--proxy-bypass-list=<-loopback>` removes the implicit bypass for `localhost`, `127.0.0.1` and `[::1]`, so loopback goes through it too. A proxy's own IP literal is mapped away by `MAP * ~NOTFOUND` like any other, so the launch adds `EXCLUDE 127.0.0.1`. Whether Chromium 154 under Playwright honors all three together waits for a run with the tools installed.
- Not yet measured here (#861): how Chromium 154 under Playwright treats `localhost` through the proxy. Kelpie dials `localhost` and `127.0.0.1` on the dev port straight on the loopback and refuses every other local address, an IPv6 literal included, so a browser that sends `localhost` to the proxy as `CONNECT localhost:<port>` or as an absolute `http` URL is served either way.
- `srt` writes `allowLocalBinding` and `allowMachLookup` from kelpie's `Policy.listen` and `services`. A dev server under Vite needs `com.apple.FSEvents` to watch files on macOS. The settings are asserted in `adapters::srt::tests`, and a running server under the real runtime is not yet measured here.
