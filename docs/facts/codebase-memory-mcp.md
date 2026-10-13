# codebase-memory-mcp

DeusData/codebase-memory-mcp, MIT per its badge, written in C. Indexing is local CPU and costs no tokens ("average repo in milliseconds", the Linux kernel in 3 minutes), stored in SQLite under `~/.cache/codebase-memory-mcp/`, and a background watcher reindexes on git changes. It has 17 MCP tools, including `detect_changes` (git diff to affected symbols) and `trace_path`. Its installer writes to agent config files, so load it through `--mcp-config` instead. Claude Code lists its tools as deferred behind a tool search: in the implementation series no worker looked them up unprompted, in 12 runs out of 12.

## Pinned for kelpie (#599)

- Pinned release `v0.11.0`. `tools install` downloads the archive for the machine (`darwin-arm64`, `darwin-amd64`, `linux-amd64-portable`, `linux-arm64-portable`) and refuses one whose SHA-256 is not the one in `src/tools/graph.rs`. Windows and other CPUs get no server.
- Kelpie starts it as an MCP stdio server with `CBM_CACHE_DIR` (the index folder) and `CBM_ALLOWED_ROOT` (the worktree) set, and indexes first with `codebase-memory-mcp cli index_repository --repo-path <worktree>`. Its README says a repo indexed from outside a session is not watched, so kelpie indexes again before every call.
- Not measured, because the sandbox the implementing worker ran in could not fetch a release asset or run the binary: the archive's layout (the installer takes the file named `codebase-memory-mcp` wherever it sits), how long a second index of a changed worktree takes, and whether the server starts inside a call's sandbox.
- Risk not measured: the README describes a shared per-account daemon that refuses a second canonical cache root while a process using the first is active. Kelpie gives each work item its own `CBM_CACHE_DIR`, so concurrent work items may find the second server unable to start. The call then runs with a failed MCP server and no graph, not a failed turn. If it happens, move to one shared cache folder per kelpie home and remove a work item's project from it by name.

