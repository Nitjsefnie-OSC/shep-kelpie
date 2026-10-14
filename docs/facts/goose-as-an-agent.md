# goose as an agent

Written 2026-10-13 for #787 (slice 3 of #784) against goose 1.54.0, the version on the maintainer's machine, read from its source at the `v1.54.0` tag. The builder had no model, so no model turn was recorded. What was run is a `goose run` against a server port nothing listens on, in the builder's sandbox, which shows the output shape, the exit code and the retry delay of a host that does not answer. #798 records real turns and corrects what differs.

## The call

- `goose run --no-profile --output-format stream-json --text=<prompt>`
  - `--text=<prompt>` is one argument, so a prompt that starts with `-` is not read as a flag.
  - `--system=<text>` carries the text of the call's instructions file. goose takes text, not a path. It is sent on every call, a resumed one too, since goose builds its system prompt afresh each time.
  - A researcher gets no `--with-builtin`, `--with-extension` or `--recipe`: the model is offered no tool. A worker gets `--with-builtin developer` (see A worker inside the fence), and so does a new session of a reviewer, advisor or retro in a fence (see A reviewer, an advisor and a retro).
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

- `GOOSE_MODE=chat` for a call that loads no extension: it has nothing for `auto` to allow, and `chat` makes goose reject a tool call that appears anyway, from an extension `--no-profile` still loaded. A worker, and any call that loads an MCP server, runs in `auto` behind the hook (see A worker inside the fence and MCP servers).
- `GOOSE_SLASH_COMMANDS_ENABLED` is left unset. It is not read on 1.54.0's default reply loop, and #792 and #793 use goose's commands.
  - goose reads a first message that starts with `/` as one of its own commands (`execute_command` in `crates/goose/src/agents/execute_commands.rs`).
  - A prompt of `/<plugin>:<skill> rest`, run against the unreachable server, reached the model call: goose passes a slash prompt that names no command of its own on to the model as text. So the adapter adds no prefix. Not checked: a prompt that names a goose command or a skill goose has. None is loaded here.

## Project files goose loads whatever `--no-profile` says

- goose discovers project plugins from `.agents/plugins` and their settings from `.config/goose` in the working folder, and `settings.local.json` there can disable any plugin by name.
- The adapter refuses a call whose working folder holds either, naming the folder. The worker's fence relies on this.

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

## A worker inside the fence

Written 2026-10-14 for #788, from goose's v1.54.0 source and docs. No model turn was run: what follows about how goose behaves is read, not measured, and #798 runs a fenced worker live.

- goose has no sandbox rules for a tool call of its own. It runs `PreToolUse` command hooks from plugins before every call and refuses the call when a hook exits 2. The reason is the hook's stderr, shown to the model as "Tool call denied by policy hook".
  - The payload on stdin has `event`, `session_id`, `matcher_context`, `tool_name`, `tool_input` and `working_dir`. The `developer` extension's tools are named bare: `shell` (`command`), `write` (`path`, `content`), `edit` (`path`, `before`, `after`), `tree` and `read_image`. goose has no file-read tool.
  - A hook that allows prints nothing on stdout.
  - `timeout` is in seconds (default 30). The command runs through `sh -c`.
- `"on_failure": "block"` makes a hook that cannot run (missing program, crash, timeout) refuse the call, goose 1.48 and later. Without it a failing hook lets the call through. That is the fail-closed finding ADR 0006 needs.
  - A `hooks.json` goose cannot parse is skipped without an error, which fails open. The plugin's file is written by kelpie, and the test reads it back as JSON.
- In `GOOSE_MODE=auto` goose ignores `permission.yaml`, so the hook is the only gate. The worker runs in `auto` with `--with-builtin developer`, which is safe on a resumed call. A repeated `--with-extension` name ends the run, so a resume passes none.
- Plugins load from `<GOOSE_PATH_ROOT>/.agents/plugins/<name>` (user) and `<working folder>/.agents/plugins/<name>` (project). A project plugin wins over a user one of the same name.
  - `<root>/config/config.yaml` (kelpie keeps any other key it holds) has `plugins: {<absolute plugin path>: {enabled: bool}}`, and an `enabled: false` entry drops the plugin.
  - `disabledPlugins` in the working folder's `.config/goose/settings.local.json` or `settings.json`, or the root's `.config/goose/settings.json`, disables a plugin by name.
- So the worker is kept from dropping the plugin three ways:
  - the plugin's name is fixed (`kelpie-fence`) and a call refuses to start when the root's `.agents/plugins` holds any other entry, the root's `.config/goose` holds a `settings.json` or `settings.local.json`, or the worktree holds `.agents/plugins` or `.config/goose`
  - `.config/goose` in the worktree is fenced as a harness folder at any depth, as `.agents` already is
  - the root's `.agents`, `.config` and `config` are outside the sandbox's `policy.write`, so the call's own `shell` cannot write them. `data`, `state` and the scratch folder stay writable.
- The guard (`kelpie guard ... --goose=<build folder>`) judges `shell` as Claude Code's `Bash`, `write` and `edit` as `Write` and `Edit` held to the worktree and the build folder (a relative `path` is the working folder's), and judges `tree` as the `ls` of its path through the command guard, so the project's `command` hooks see it. `read_image` and every other name, `delegate` and `load` included, are refused, and so is a payload it cannot read.
- goose cannot run an `after_command` hook. Its `PostToolUse` payload names the tool and its input but not the result, and a hook's output is never read back by the model, so `kelpie guard --after` has nothing to judge and nowhere to report. A goose worker beside one is refused at load.
- A vote resumes a worker's session under another settings file (`vote-settings-<n>.json`). goose's files are keyed on the settings path, so the resume finds a root with no session and returns `NoSession`, as pi and Codex do. A retro under it is no different: `prepare` writes its plugin, and the run is `NoSession`.

## A reviewer, an advisor and a retro

Written 2026-10-14 for #789, from the same source reading as the worker's. No model turn was run.

- goose has no read tool, only `shell` and `tree`, so a reviewer, advisor or retro in a fence is offered the `developer` builtin behind the fence's plugin, as Codex is given a shell. `GOOSE_MODE=auto`, as the worker's, since `chat` would reject the tool calls.
- The plugin's command is `kelpie guard <common dir> <worktree> [--hooks=...] --goose=<build folder> --read-only`. `shell` runs only what `kelpie guard --read-only` lists (the Codex list, `src/guard/reading.rs`, no git, no redirect), and the project's `command` hooks judge what it lets through. `tree` is the `ls` of its path through the same guard. `write`, `edit` and every other tool name are refused with "this call writes no file".
- The reviewer's fence has the worktree as code and an empty folder of its own, as a Codex reviewer's does, and no write folder. goose has no way to write a test into a folder, so a goose reviewer is never asked for tests.
- A retro and an advisor run under a fence that can write (the worker's, or the advisor's with none). The sandbox's `write` is cleared for both, so the hook is the first layer and the sandbox the second. goose's own data, state and scratch folders stay writable.
- A resumed session is given no `--with-builtin`: goose restores a session's own extension set, and a retro resumes the worker's session. Whatever that set holds, `shell`, `write` and `edit` pass the hook and are refused there. A resumed worker is still given the builtin.
- A call with no fence, and the researcher in any fence, is offered no extension and runs in `GOOSE_MODE=chat`.
- A reviewer's prompt names Read, Grep and Glob, so goose's gets one more paragraph naming the shell, `tree` and the commands it runs. The advisor's and the retro's prompts name no such tool.
- Not run: a reviewer reading a repo through the shell on a real model, and whether a resumed session keeps `developer` in its restored set. #798 records a reviewer.

### Risks for #798 to measure

- `config.yaml` is read only to the sandbox, and written by `prepare` with the plugin already on. Not run: whether goose starts, or later writes the file back (a first-run setup, a migration), under a read-only config. If it cannot, the file alone becomes writable and the risk moves to the fence: a write there could switch the plugin off.
- Not run: the hook as goose runs it. The plugin's `hooks.json` shape and the exit-2 refusal are read from the source. #798 records a worker whose `write` into `.claude/` and `shell` of `gh pr merge 1` are refused.
- Not run: `CONTEXT_FILE_NAMES=["AGENTS.md","CLAUDE.md"]` loading the repo's `CLAUDE.md`.

## MCP servers

Written for #794, from goose's v1.54.0 source. No model turn was run: #798 runs a real server.

- goose reads no `.mcp.json` from the repo. A plugin root's `.mcp.json` is read on a new session without `--no-profile` only, so kelpie passes every server as a flag (`src/adapters/goose/mcp.rs`).
  - stdio: `--with-extension "<name>:KEY=value command args"`, with the server's name as Claude Code names it.
  - HTTP with no headers: `--with-streamable-http-extension <url>`. The flag takes no header and no env, and goose has no SSE type.
- The string is split like a shell (`split_command_args` in `crates/goose/src/utils.rs`): double quotes group, a single quote counts only at the start of a word, and there is no escape.
  - A word with whitespace, or one that starts with `'`, is written in double quotes. A value holding a double quote cannot be written, so the entry is dropped with a note.
  - An env pair is `KEY=value` before the command. A command holding `=` would read as one, so it is refused.
- An explicit name needs two or more of `A-Za-z0-9_-`. The name `developer` is the builtin's, so a stdio server of that name, or an HTTP server whose URL derives to it, is refused. An HTTP server is named from its URL (host, `_port` if not the scheme's default, `_path`) and cannot be renamed. The module computes that key (`name_to_key` in `crates/goose/src/config/extensions.rs`) because the hook needs it, and refuses a URL whose key it cannot be sure of (userinfo, IPv6, an odd host, dot segments).
- Tools are named `<name lowercased>__<tool>`, hyphens kept. The hook (`kelpie guard ... --goose-server=<key>`, once per loaded server) lets through a name that is a key, `__`, and a tool. The name Claude Code gives an HTTP server is not goose's, so the hook is handed goose's key.
- goose skips an env key such as `PATH`, `LD_PRELOAD` or `NODE_OPTIONS` with a warning (`Envs::DISALLOWED_KEYS` in `crates/goose/src/agents/extension.rs`). kelpie leaves those keys out of the string and notes them, and the server is kept.
- A session stores its extension set. A resumed run starts from the stored set, then adds the flags again (`collect_extension_configs` in `crates/goose-cli/src/session/builder.rs`), and the same explicit name twice ends the run with exit 1. So a new session passes the flags and a resumed one passes none.
  - The set is fixed when the session is created. A server added after the session began is not seen by it, and goose cannot remove one.
  - kelpie records the keys a new session is created with (`<root>/servers/<session id>.json`, which the call cannot write). A resumed call's hook and `GOOSE_MODE` follow that record, not the set the call loads by then, so a server renamed or removed on `main` stays allowed for the session that holds it and a new one is not. A session with no record holds none. A retro resumes the worker's session but is handed no server, so its hook names none: goose still restores the set, and the hook refuses its tools.
- Two servers whose keys are alike (`Docs` and `docs`, or a URL's key and a stdio name) would end the run. Only the first loads, in the order of the flags with the code graph last, and a repo server never replaces it, whether it shares its name or only its key.
- The sandbox reads the code graph's binary and index and writes only its index (`fence_the_graph`), as for the other harnesses. A host a server reaches is still named in `worker.allowed_domains`, or `researcher.allowed_domains`.
- Not run: the flags against a real server, the `tool_name` in the hook's payload (read as `<key>__<tool>` from goose's source), and whether goose warns, not fails, on a skipped env key.

## The idle watch and the watch

Written for #790. The event shapes are from goose's source, not a recorded turn. #798 checks them.

- goose keeps its sessions in SQLite under `GOOSE_PATH_ROOT`, which kelpie cannot tail. The call's stdout file (`<settings>.stdout.jsonl`) is what it reads. `File::create` empties it as each call spawns (`src/adapters/process.rs`), so a resumed turn does not inherit the last turn's events.
- The idle watch (`last_active`) is `Nothing` until the file holds a `message`, `notification` or `error` line, then the file's write time. The banner and `complete` do not count. A file past 1 MiB counts unread, a last line without its newline is not an event, and a missing file is `Nothing`.
- goose does not stream text: a whole assistant message is one line. A model generating a long answer or a long reasoning block shows no event until it ends, so a slow model can read as silent. The idle watch is coarser on goose than on pi, and `agents.fallback_after` is 10 minutes by default.
- The watch finds the file through the `session-started` step's `transcript.folder`, which for goose is the call's goose root (`<settings>.goose`). The stdout file is the root's sibling with `.stdout.jsonl` for the extension. The step keeps its shape, so an older log reads. Chosen over scanning the settings folder because that folder holds every role's calls and names no session.
- A settings path is reused by every call of its role and issue, so the file is not a session's history. The watch reads a path's newest session only: a `session-started` that names a root already named drops the earlier goose session, and a resumed session reads the emptied file from its start.
- A `message` line has `message.created` in seconds, which is its time. Its content blocks the watch renders: `text` (said, or prompt for a `user` message), `thinking`, `toolRequest` (`toolCall.value.name` and `arguments`) and `toolResponse` (`toolResult.value.content`, with `isError` or `status: "error"` shown as `error: ...`).
- An `error` event has no time, so the watch gives it the time of the line before it in the file, or the call's start when it is the first. It is shown as a `result` line `error: <text>`.
- `transcripts::format_of(Harness::Goose)` stays `None`. No file holds a whole session, since the stdout file is emptied per call, so the researcher's panel gets no worker-session text on goose.

## Fixtures

- `fixtures/goose-run-unreachable.jsonl` is goose's own output from the closed-port run, the temporary folder renamed.
- `fixtures/goose-run-fresh.jsonl`, `goose-run-resumed.jsonl` and `goose-run-max-turns.jsonl` are written by hand from the event types, shaped after that output. `fixtures/goose-run-refused.jsonl` is a worker whose `write` into `.claude/` and `shell` of `gh pr merge 1` are refused, also by hand, with the shape of a tool request and its response read from the source. #798 replaces them with recordings, made by the ignored tests `adapters::goose::tests::record_a_turn_a_resumed_turn_and_an_unreachable_server` and `record_a_worker_whose_write_and_shell_the_fence_refuses`.

## The terminal session

Written for #796. Read from goose 1.54.0's source and not run. #798 confirms it live.

- `goose session --resume --name <id>` resumes a session in the terminal. `--session-id` takes goose's own id for the session (`get_session(session_id)` in `crates/goose-cli/src/session/builder.rs`), which is not the name kelpie creates it with, so kelpie resumes by `--name`, which matches a name or an id. `--session-id` without `--resume` is an error, and `session` has no flag for a first message.
- The terminal's tokens add to the session's accumulated totals in `sessions.db`, which the next `complete` event reports (`get_session_usage_totals`, a sum over the session and its children). Kelpie does not read the database, so it marks its own record of the totals stale when the terminal ends, and the next headless turn is charged none of its tokens.
- A resumed session uses the extension set it stored, so the command passes no `--with-*` flag and no `--no-profile`.
- The session's permission mode is set again from `GOOSE_MODE` at every start, so the terminal can run in a different mode than the headless calls. Kelpie runs it in `approve`, where goose asks before each tool call. The `PreToolUse` hook runs in any mode.
- The telemetry consent dialog is skipped when `GOOSE_TELEMETRY_OFF` is set, so no prompt blocks the start.
- If the terminal's folder is not the session's, goose asks on the terminal whether to switch, so the command runs in the worktree.
- goose keeps its sessions in SQLite with no per-session lock. Attach's wait for a call in flight and its hold keep a headless call and the terminal apart.
- Not run: that interactive goose runs inside kelpie's sandbox with a terminal, and which paths its history file needs. Its history goes under the session's `GOOSE_PATH_ROOT`; a path the sandbox refuses goes in `policy.write`.

## Not run yet (for #798)

- An MCP server's flags and tool names, as in MCP servers above.
- A model turn: the reply text, the `complete` totals and the shape of an `error` event.
- That `complete` is cumulative across `--resume`, and whether its input includes the cache.
- The turn-limit message.
- The request path under `OPENAI_BASE_URL` against the forwarder's chat-completions-only filter.
- That goose honours the sandbox's proxy for the forwarder's host.
- That goose starts without writing `<root>/config`.
- Which session `--resume --name` picks when two share a name.
- The retry delay on a refused connection and on a 502 or 503.
- That `goose session --resume` starts under the sandbox with a terminal, as in The terminal session above.
