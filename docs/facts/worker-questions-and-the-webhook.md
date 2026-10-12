# Worker questions and the webhook

Measured 2026-09-27 on Claude Code 2.1.283 and the system's curl.

- Sonnet 5 at medium effort, given kelpie's worker instructions and an issue that left a choice to the maintainer, ended its turn on a well-formed question block, 3 runs of 3. One run's issue matched the instructions' example, and it asked the example's question word for word. One run noticed it was not in a real project and asked about that instead. The third is recorded in `fixtures/claude-p-question.json`.
- `curl --config -` takes the URL, headers and body from stdin, so no process listing shows the URL. With nothing listening, curl exits 7; an HTTP error still exits 0, and `--write-out '%{http_code}'` gives the status.
- A sandboxed worker cannot reach kelpie's shepherd socket, so it cannot answer its own ruling with `shep trigger`.
  - Read from Claude Code's [settings reference](https://code.claude.com/docs/en/settings-reference#sandbox-network-allowunixsockets) on 2026-09-27, not measured: with `sandbox.network.allowUnixSockets` unset the macOS sandbox blocks every Unix socket, and `allowAllUnixSockets` defaults to false. `enableWeakerNetworkIsolation` opens only `com.apple.trustd.agent`.
  - The worker's settings set neither socket key, and a test pins that. On Linux and WSL2 the block is the sandbox's seccomp filter, and without the filter Unix sockets are not blocked.
- Discord refuses a message over 2,000 characters (from its docs, not measured). ntfy's docs give its limit as 4,096 bytes, but measured on ntfy.sh on 2026-09-29 a body of exactly 4,096 bytes already arrived as `attachment.txt`, and 4,095 stayed text, so kelpie's ntfy limit is 4,095. Kelpie cuts a long question in the middle, keeping its start and the triggers at its end.
