# Compaction

When a worker's session compacts, on each harness. Runner loop and turns are in workers.md.

## When and who

- Session whose last turn passed 150k context compacts before next turn (`WORKER_COMPACT_PAST`, `src/runner/compact.rs`). Claude Code compacts mid-turn at `worker.compact_window` (200000).
- A compaction a harness does by itself mid-turn counts in `kelpie_compactions_total` (metrics.md); kelpie's own `COMPACT` is a call of kind `compact`.
- A harness reports its session's context in its reply, and a `COMPACT` call compacts the session. Each harness does both its own way, below.

## pi

- Its reply's context is the last request's prompt. A `COMPACT` call sends the `/kelpie-compact` command of an extension kelpie writes. pi's own threshold compaction still runs, and on a window under 150k only it does. #760
- `worker.compact_window`: `settings.json` in the call's pi home sets `compaction.reserveTokens` to the model's `context` less the window, only when above pi's default 16384 (else no file). The extension hands the turn's prompt back (`<settings>.prompt.md`, `sendMessage` steered) after a `threshold` or `overflow` compaction, not a `manual` one, which gets the runner's reminder. `src/adapters/pi/compact.rs`. #761

## Codex

- Compacts at the window; `COMPACT` lowers the next turn's limit under its context. `src/adapters/codex/context.rs`. #770

## goose

- A worker's reply has its context from `usage.input_tokens` of its session in `goose session list --format json`, run after the turn in the call's sandbox and root. A failed read is logged and leaves it unset. Other roles read none. The session is found by `id`, else by `name` (kelpie's session id is the `--name`, and goose picks an `id` of its own).
- A `COMPACT` call resumes the session and sends goose's own `-t /compact`, which answers the assistant text `Compaction complete`; any other text is a failed call carrying it. Its usage is the session's totals after less before, so the compaction's request counts.
- `worker.compact_window`: `GOOSE_AUTO_COMPACT_THRESHOLD` is window / `context` rounded up to 4 decimals, set only under goose's own 0.8 (1 or more turns it off, so never set). goose checks it at turn start, from the last request's total, so it compacts up to one completion past the window.
- Inside one turn goose compacts only after a context-exceeded error, twice at most. A compaction keeps the latest user message, so nothing is handed back. A long first turn on a small window relies on that recovery. A turn another image adopts reports no context. `src/adapters/goose/compact.rs`. #792
