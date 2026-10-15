# MCP servers in a call's transcript

What Claude Code and pi write down of a call's MCP servers, which `kelpie_mcp_servers_total` and `kelpie_mcp_tool_calls_total` are read from (#922). `src/mcp_use.rs`

## Claude Code (2.1.300 and 2.1.301, 2026-10-15)

- `--output-format json` has no init line, so the transcript is the only place a headless call's servers show. It writes how they stand in an `attachment` line of type `deferred_tools_delta`, whose `failedMcpServers` is a list of `{name, errorCode, error}` and whose `pendingMcpServers` and `needsAuthMcpServers` are lists of names.
- The line is written at a session's first call, and again only when the servers stand differently. A resumed call whose server still failed wrote none (measured on 2.1.301, with a server whose command is not there), so the last such line in the whole transcript is how they stood at the end of the call. A later line with an empty `failedMcpServers` ends a failure.
- A tool call is a `tool_use` block named `mcp__<server>__<tool>`, the server's name with every character but letters, digits, `_` and `-` as `_` (`claude.ai Gmail` is `claude_ai_Gmail`).
- On the maintainer's machine, 220 worker transcripts hold a failed server: `shadcn` 214 (`CONNECTION_CLOSED`), `supabase` 18, a project's own docs server 14, `codebase-memory-mcp` 8, and a plugin's server 2 (`ENOENT`, its command not found). A transcript can hold more than one.
- No worker transcript on that machine has a `mcp__codebase-memory-mcp__*` tool call. The fixture with one, `fixtures/claude-transcript-mcp-tool-calls.jsonl`, was recorded for #922 in a shep-kelpie worktree, with the code graph and a server whose command is not there.

## pi (1.1.0, 2026-10-15)

- pi's session file records no MCP server that failed to connect. A server whose command is not there (`ENOENT`) shows in `pi mcp list` and nowhere in the session file, so kelpie counts only pi's tool calls.
- A server that connected shows only by its tools: with `exposure: direct`, which kelpie sets, they are in the `toolsAdded` of the session's `system` message. A tool call is a `toolCall` block in an `assistant` message, named `mcp__<server>__<tool>` with every character but letters, digits and `_` as `_` (`mcp__codebase_memory_mcp__list_projects`).
- Kelpie's pi calls on the maintainer's machine wrote 156 session files, 61 of them shep's. Five shep calls were given the code graph: none had its tools in `toolsAdded` and none called it.
- No pi session kelpie wrote has an MCP tool call, so `fixtures/pi-session-mcp.jsonl` was recorded for #922 from pi 1.1.0 against a stand-in model that called the code graph's `list_projects`, with the server connected.
- Why the five calls had no code graph tools (#935): pi's sandbox gets the code graph's grants from the same function as Claude Code's, `fence_the_graph` in `src/adapters/mcp.rs`, and the grants #918 added (the runtime folder writable, and its socket in `allowUnixSockets`) are in pi's policy, as `adapters::pi::tests` asserts, down to the `allowUnixSockets` srt receives beside pi's forwarder. Before #918 no harness's sandbox let the server start its daemon, so the server closed its connection, which is what the 8 failed `codebase-memory-mcp` entries in Claude Code's transcripts show too. pi records no failed server, so for pi that shows only as missing tools. Not measured: the five calls' dates against #918's merge, and a pi call on a project with the code graph since #918, because a worker's sandbox cannot run pi. `kelpie_mcp_tool_calls_total{server="codebase-memory-mcp"}` for pi stays at 0 if that call still fails.
- pi connects its servers in the background and the first prompt waits only 10 seconds for those with `direct` tools (`DEFAULT_STARTUP_WAIT_MS` in pi 1.1.0's `extensions/mcp/index.js`). A server that connects later has its tools available later, so a cold daemon start slower than that would leave them out of the first request. Not measured.
