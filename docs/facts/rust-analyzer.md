# rust-analyzer

Whether a code-intelligence (LSP) server helps a worker on this repo (#929).

- Before the maintainer turned on Claude Code's `rust-analyzer-lsp` plugin on 2026-10-15, none of 6,477 Claude Code transcripts on the maintainer's machine had an LSP tool call or an injected diagnostic.
- A worker cannot reach that plugin anyway: `--setting-sources project` (`crates/adapters/src/adapters/claude/mod.rs`) drops the maintainer's plugins.
- Offered to pi on the local model as an MCP server for a 59-minute and a 75-minute arm of the refactor matrix (2026-10-17), it was called 0 times in 637 tool calls. The table is in [codebase-memory-mcp](codebase-memory-mcp.md#offered-to-the-local-model-called-almost-never-600-measured-2026-10-17).
- Not measured: rust-analyzer's cold start and memory in a worker's sandbox, since no worker called it.

So kelpie does not give workers an LSP server. `cargo clippy`, which every worker runs before it hands over, already gives the diagnostics.
