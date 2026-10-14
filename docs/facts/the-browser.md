# The browser

What was measured behind a worker's browser outside its sandbox (ADR 0011, shep-pm/shep-kelpie#858). Most entries were recovered from the removed bridge's reviews and the experiments repo, in git history at `69e0f710^`. Where the recovered notes name no date, the entry says so.

- Headless Chromium cannot start under `srt` 0.0.77 (2026-09-30, macOS). Its profile allows no `mach-register`, and Chromium dies with `bootstrap_check_in ... MachPortRendezvousServer: Permission denied (1100)`. So the browser runs outside the sandbox.
- Chromium's `--host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE <host>"` also blocks IP literals and WebSockets to a host it maps away (2026-09-29, Chromium 154). So one flag holds the browser to the listed domains, with no proxy.
- The Playwright MCP server's own `file:` block does not hold: `view-source:file://` read any file past it. Kelpie's filter passes a URL only as `http` or `https` to a listed host. No date recorded.
- The server's `--allowed-origins` is not a security boundary, so kelpie does not rely on it. No date recorded.
- The server's `checkFile` (Playwright 1.64) resolves symlinks, so its `--output-dir` must be a folder the worker cannot write. No date recorded.
- A filename check followed by a later write let a swapped folder write outside the worktree: a review raced a folder against a link. So every `filename` is refused, and a screenshot comes back in the tool's reply. No date recorded.
- A worker that ends a turn on a background task is never woken under `claude -p`. So the instructions say to wait in the foreground. No date recorded.
- A socket listed in `allowUnixSockets` connects even from inside an unreadable folder. A leftover process of another work item cannot reach a call's socket, since each work item's name is a 128-bit random one. One of the same work item can, since its turns share the name; it gets the same worker's filtered browser. No date recorded.
- Raw CDP's network-idle event never fired with Vite open (30 s timeout), while Playwright's `networkidle` settled in about 1.1 s. No date recorded.
- `kelpie tools install` took 11 s with a warm npm cache, with `@playwright/mcp` and Chromium. No date recorded, and a cold cache takes longer.
