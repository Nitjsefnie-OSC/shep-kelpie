# codebase-memory-mcp

DeusData/codebase-memory-mcp, MIT per its badge, written in C. Indexing is local CPU and costs no tokens ("average repo in milliseconds", the Linux kernel in 3 minutes), stored in SQLite under `~/.cache/codebase-memory-mcp/`, and a background watcher reindexes on git changes. It has 17 MCP tools, including `detect_changes` (git diff to affected symbols) and `trace_path`. Its installer writes to agent config files, so load it through `--mcp-config` instead. Claude Code lists its tools as deferred behind a tool search: in the implementation series no worker looked them up unprompted, in 12 runs out of 12.

## Pinned for kelpie (#599)

- Pinned release `v0.11.0`. `tools install` downloads the archive for the machine (`darwin-arm64`, `darwin-amd64`, `linux-amd64-portable`, `linux-arm64-portable`) and refuses one whose SHA-256 is not the one in `src/tools/graph.rs`. Windows and other CPUs get no server.
- Kelpie starts it as an MCP stdio server with `CBM_CACHE_DIR` (the index folder) and `CBM_ALLOWED_ROOT` (the worktree) set, and indexes first with `codebase-memory-mcp cli index_repository --repo-path <worktree>`. Its README says a repo indexed from outside a session is not watched, so kelpie indexes again before every call.
- Not measured: how long a second index of a changed worktree takes.

## The daemon (measured on macOS, v0.11.0, 2026-10-15)

- The server and its CLI talk to a daemon, which the first one to need it starts and which stops when its last client disconnects. There is one daemon per runtime folder, `CBM_RUNTIME_DIR` (default `/private/tmp/cbm-daemon-<uid>` on macOS), and it serves one cache folder. Many repos can share one cache. A second cache under the same runtime folder fails with "active account daemon uses a different cache directory". A separate `CBM_RUNTIME_DIR` per cache runs side by side.
- In production this broke every index from the moment an interactive Claude Code session started the default daemon without kelpie's `CBM_CACHE_DIR`. Kelpie now gives each work item its own runtime folder, `<project>/cbm/<issue>`, so no other session's daemon can hold it.
- The runtime folder must exist (the server will not create it). Its mode does not matter.
- The socket is `<runtime>/cbm-daemon-<uid>/cbm-<16 hex>.sock` and its path must fit the 104-byte macOS limit. A longer one fails with "secure CLI coordination could not be created (endpoint)", which names no path, so kelpie adds the path to a failed index's error when it is too long.
- Inside a call's sandbox the server has to start its own daemon, and `socket_bind` fails with EPERM unless the runtime folder is writable and listed in `network.allowUnixSockets`. With both, it starts. The folder alone in `allowUnixSockets` is enough, but it must be the real path (`/private/tmp`, not `/tmp`). Not measured on Linux, where srt refuses every Unix socket (#488).
- `ui_enabled` defaults to true and is saved per cache folder. With it on, the daemon serves a web UI on `127.0.0.1:9749`. Kelpie runs `config set ui_enabled false` with the work item's `CBM_CACHE_DIR` before each index, and the index then runs with no UI.
- `cli delete_project --project <name>` removes one project from a cache.
