# goose as an agent

Written 2026-10-13 for #787 (slice 3 of #784) against goose 1.54.0, the version on the maintainer's machine, read from its source at the `v1.54.0` tag. The builder had no model, so no model turn was recorded. What was run is a `goose run` against a server port nothing listens on, in the builder's sandbox, which shows the output shape, the exit code and the retry delay of a host that does not answer. #798 records real turns and corrects what differs.

## The call

- `goose run --no-profile --output-format stream-json --text=<prompt>`
  - `--text=<prompt>` is one argument, so a prompt that starts with `-` is not read as a flag.
  - `--system=<text>` carries the text of the call's instructions file. goose takes text, not a path. It is sent on every call, a resumed one too, since goose builds its system prompt afresh each time.
  - No `--with-builtin`, `--with-extension` or `--recipe`: the model is offered no tool.
- `call.effort` is not passed. goose's openai provider has no reasoning-effort flag or variable.
- Session identity: goose picks its own session ids and `stream-json` never prints one.
  - A name set with `--name` is kept as given (`user_provided_name` in `crates/goose-cli/src/cli.rs`), and `--resume --name` finds the session whose name or id matches.
  - So kelpie's session id is the goose session name. A new session is `--name <id>`, a resumed one `--resume --name <id>`. There is no thread file.
  - A resume that names a session goose does not have prints "No session found with name" on stderr and exits 1.
  - Not measured: which session `--resume --name` picks when two share a name. A new session started again under a name already in use (an unreachable retry run as new) would make two.
- Variables, set through `env NAME=value ... goose`:

| Variable | Value | Why |
|---|---|---|
| `GOOSE_PATH_ROOT` | absolute path of `<settings>.goose` | moves goose's `config`, `data` and `state` into the call's own root. A relative path is ignored. |
| `GOOSE_PROVIDER` | `openai` | the provider for an OpenAI-compatible server |
| `GOOSE_MODEL` | the call's model | |
| `GOOSE_CONTEXT_LIMIT` | the server's `context` | goose cannot ask an OpenAI-compatible server for it |
| `OPENAI_BASE_URL` | the forwarder's worker URL | the sandbox allows no model host, so goose dials the forwarder's host |
| `OPENAI_API_KEY` | `kelpie` | a placeholder. The forwarder adds the gateway's real key, so no file or variable of the call holds it. |
| `GOOSE_DISABLE_KEYRING` | `1` | goose stores nothing in the keychain |
| `GOOSE_TELEMETRY_OFF` | `1` | |
| `TMPDIR` | the call's scratch folder | the one temporary folder the sandbox lets it write |
| `GOOSE_MODE` | `chat` | see below |

- `GOOSE_MODE=chat`, not `auto`, for this slice. A call that loads no extension has nothing for `auto` to allow, and `chat` makes goose reject a tool call that appears anyway, from an extension `--no-profile` still loaded. #788 sets `auto` for the worker.
- `GOOSE_SLASH_COMMANDS_ENABLED` is left unset. It is not read on 1.54.0's default reply loop, and #792 and #793 use goose's commands.
  - goose reads a first message that starts with `/` as one of its own commands (`execute_command` in `crates/goose/src/agents/execute_commands.rs`).
  - A prompt of `/<plugin>:<skill> rest`, run against the unreachable server, reached the model call: goose passes a slash prompt that names no command of its own on to the model as text. So the adapter adds no prefix. Not checked: a prompt that names a goose command or a skill goose has. None is loaded here.

## Project files goose loads whatever `--no-profile` says

- goose discovers project plugins from `.agents/plugins` and their settings from `.config/goose` in the working folder, and `settings.local.json` there can disable any plugin by name.
- The adapter refuses a call whose working folder holds either, naming the folder. #788 relies on this for the worker.

## The sandbox

- Writable: `<root>/data`, `<root>/state`, the scratch folder. Readable: the root, the scratch folder, the call's stdout and stderr files, its instructions, its plugin folders and `reach.read`.
- `<root>/config` is made by `prepare` and read only. Not run: whether goose starts under the sandbox without writing it. If it cannot, #798 records that and the next slice allows it.
- Every harness's credentials are unreadable, goose's own included: the call logs in nowhere, and its config is relocated.
- goose's calls reach only the forwarder's socket through the sandbox's proxy. Not run: whether goose honours the proxy variables the sandbox sets for the host it dials.

## stdout

- goose prints a banner on stdout first (the duck and "goose is ready"), then one JSON object a line. Only lines that start with `{` are read.
- Each is tagged `type`: `message`, `notification`, `error` or `complete`.
  - A `message` has `message.role` and `message.content`, a list of blocks. The reply text is the text blocks of the last `assistant` message.
  - A `message` event carries no tokens on 1.54.0: the CLI drops goose's usage event.
  - A `complete` event has `total_tokens`, `input_tokens`, `output_tokens`, `cache_read_input_tokens` and `cache_write_input_tokens`.
- The `complete` figures are the session's running totals across calls, read from the source. A call's usage is the totals after it less the totals before it, as Codex's `Thread.spent` is.
  - Kelpie keeps the totals in `<root>/spent/<session id>.json`. A missing file is zero, and a new session resets it to zero.
  - Cached input is counted within the input, as OpenAI counts it, so the cache fields are taken out of `input` and reported as `cache_read` and `cache_write`.
  - Not measured: the cumulative nature of `complete` and whether `input_tokens` includes the cached tokens, both read from the source only.
- No price (`session_cost: None`) and no context figure (`context: None`).

## Errors

- A model host that does not answer: the run against a closed port printed an assistant message "Network error: Could not connect to 127.0.0.1:9 ..." and a `complete` event of zeros, and exited 0.
  - The brief expected exit 1 and an `error` event. Both are handled. A reply that starts "Network error:" with no tokens spent in the call is an error, and a reply that starts so but spent tokens is the model's own words.
  - An error that says the host did not answer (connection refused or reset, could not connect, 502, 503) is `Unreachable`, and any other is `Failed`. An error that leads with another status is the call's own. The runner's unreachable wait does the rest.
  - An error carries the last 2048 bytes of its text.
  - Not measured: the `error` event's field name. The adapter reads `error`.
- goose retries a provider error before it reports one. The run against a closed port took about 7.5 seconds to print its message, so a failure is seconds late.
- A message that starts "I've reached the maximum number of actions" means goose hit its turn limit and exited 0. The adapter returns `Failed` saying so. Not run: the message text on a real turn limit.
- Output that cannot be read, or an exit 0 with no assistant message, is `Unreadable`.

## The idle watch and the watch

Written for #790. The event shapes are from goose's source, not a recorded turn. #798 checks them.

- goose keeps its sessions in SQLite under `GOOSE_PATH_ROOT`, which kelpie cannot tail. The call's stdout file (`<settings>.stdout.jsonl`) is what it reads. `File::create` empties it as each call spawns (`src/adapters/process.rs`), so a resumed turn does not inherit the last turn's events.
- The idle watch (`last_active`) is `Nothing` until the file holds a `message`, `notification` or `error` line, then the file's write time. The banner and `complete` do not count. A file past 1 MiB counts unread, a last line without its newline is not an event, and a missing file is `Nothing`.
- goose does not stream text: a whole assistant message is one line. A model generating a long answer or a long reasoning block shows no event until it ends, so a slow model can read as silent. The idle watch is coarser on goose than on pi, and `agents.fallback_after` is 10 minutes by default.
- The watch finds the file through the `session-started` step's `transcript.folder`, which for goose is the call's goose root (`<settings>.goose`). The stdout file is the root's sibling with `.stdout.jsonl` for the extension. The step keeps its shape, so an older log reads. Chosen over scanning the settings folder because that folder holds every role's calls and names no session.
- A settings path is reused by every call of its role and issue, so a watch on an older session of that path reads the newer call's lines. The file is not a session's history.
- A `message` line has `message.created` in seconds, which is its time. Its content blocks the watch renders: `text` (said, or prompt for a `user` message), `thinking`, `toolRequest` (`toolCall.value.name` and `arguments`) and `toolResponse` (`toolResult.value.content`, with `isError` or `status: "error"` shown as `error: ...`).
- An `error` event has no time, so the watch gives it the time of the line before it in the file, or the call's start when it is the first. It is shown as a `result` line `error: <text>`.
- `transcripts::format_of(Harness::Goose)` stays `None`. No file holds a whole session, since the stdout file is emptied per call, so the researcher's panel gets no worker-session text on goose.

## Fixtures

- `fixtures/goose-run-unreachable.jsonl` is goose's own output from the closed-port run, the temporary folder renamed.
- `fixtures/goose-run-fresh.jsonl`, `goose-run-resumed.jsonl` and `goose-run-max-turns.jsonl` are written by hand from the event types, shaped after that output. #798 replaces all four with recordings, made by the ignored test `adapters::goose::tests::record_a_turn_a_resumed_turn_and_an_unreachable_server`.

## Not run yet (for #798)

- A model turn: the reply text, the `complete` totals and the shape of an `error` event.
- That `complete` is cumulative across `--resume`, and whether its input includes the cache.
- The turn-limit message.
- The request path under `OPENAI_BASE_URL` against the forwarder's chat-completions-only filter.
- That goose honours the sandbox's proxy for the forwarder's host.
- That goose starts without writing `<root>/config`.
- Which session `--resume --name` picks when two share a name.
- The retry delay on a refused connection and on a 502 or 503.
