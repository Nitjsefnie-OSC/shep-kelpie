# Requests in a call's transcript

What Claude Code and pi write down of each request a call made to its model, which `kelpie_request_context_tokens` and `kelpie_cache_write_tokens_total` are read from (#924). `src/request_use.rs`

## Claude Code (2.1.300 and 2.1.301, 2026-10-15)

- A request is written as one `assistant` line for each block of its reply (`thinking`, `text`, each `tool_use`). Every line holds the request's whole `message.usage` and the same `message.id` and `requestId`. A worker's session of 5 requests held 12 such lines, so a count of lines is not a count of requests, and a sum of their usage counts each request's context more than once.
- The context a request sent is `input_tokens` + `cache_read_input_tokens` + `cache_creation_input_tokens`. The cache write is `cache_creation_input_tokens`, split into `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` (Claude Code wrote 1 hour on all of them on the maintainer's machine).
- A line's `timestamp` is when its block was written, a fraction of a second to some seconds into the reply. The first line of a request is the closest the transcript has to when it was sent. A newer line of type `api-request` holds the send time, and older versions have none, so kelpie reads the first line.
- A reply Claude Code makes itself, with `model` `<synthetic>`, holds zero in every usage field. 15 of 29,825 assistant lines in 400 of the maintainer's transcripts. None of the 29,825 was a sidechain (`isSidechain` true), and kelpie skips one if it meets it, since its context is not the session's.
- A resumed call appends to the session's file. The request before a resumed call's first one is in the same file, from the call before, so a gap between calls reads as a gap: `fixtures/claude-transcript-mcp-failed.jsonl` is three calls an hour or more apart, whose requests wrote 13,152, 63,745 and 70,796 tokens, the second and third after a gap.
- `fixtures/claude-transcript-request-sizes.jsonl` is 12 lines of 6 requests from a kelpie worker's session on Claude Code 2.1.301, with the content of every block and every field but the request's own left out.

## pi (1.1.0, 2026-10-15)

- A request is one `message` entry with `role` `assistant`. Its `message.usage` has `input`, `cacheRead` and `cacheWrite`, and its entry's `timestamp` is ISO 8601 like Claude Code's.
- Every pi session on the maintainer's machine is on a local model, and none reported a cache write. `fixtures/pi-session-mcp.jsonl` (two requests, no cache) is the recording, and the tests move its second request's time and usage to read a gap.
