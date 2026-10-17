# Tool errors in a call's transcript

How Claude Code and pi write down a tool call that failed, and how often each kind happened in kelpie's calls on the maintainer's machine, for #954. Counted from 2026-10-13 to 2026-10-16 (Claude Code 2.1.300 and 2.1.301, pi 1.1.0), shep's and kelpie's own calls and the other projects kelpie ran.

## How each harness records one

- Claude Code: a `tool_result` block with `is_error: true` in a `user` message, its `tool_use_id` naming the `tool_use` block that made the call.
- A hook's refusal in Claude Code reads `PreToolUse:<tool> hook error: [<the hook's command>]: <its reason>`, so the hook that refused is in the text. Kelpie's own two are its binary first and its program second, `'<kelpie>' 'guard' ...` and `'<kelpie>' 'confine' ...`, and any other hook is its own command, which can name `guard` for some other program (`/usr/local/bin/safety guard --strict`).
- pi: a `toolResult` message with `isError: true`, its `toolCallId` naming the `toolCall` block. A refusal by kelpie's guard extension is the guard's reason behind `kelpie guard: `, which nothing else writes. Before #968 it was the reason alone, with nothing naming the guard, and a blocked result carrying no `durationMs` where one that ran does was the only thing that told them apart: pi's shape, and no marker kelpie owns.
- Those two shapes are what the reader uses, beside the mark. A call the extension blocked has its refusal as the result's only text block and no `durationMs`, and a command it refused after it ran has the reason as a block of its own at the end, after the command's own output block. A command's output is one block, so output that starts with the mark or quotes it is not a refusal.
- pi's `details` is not a marker: a bash result has `details: {}` when the guard refused it, and also when the command timed out or ran and failed.
- A failed result's text often holds output that quotes an error without being one: a `grep` of this repo's docs, or a file read with `cat`. A pattern matched anywhere in the text counted those, so each kind below matches an error line (`^error: cannot lock ref`, a line naming `/tmp/` and `Operation not permitted`), not a phrase anywhere.
- One failed command can print the same error on hundreds of lines (15 failed `cp` commands printed 621 `Operation not permitted` lines, one of them 304), so a count is of failed results, not of lines.

## How often (2026-10-13 to 2026-10-16)

| kind | Claude Code | pi |
|---|---|---|
| `guard-block` (kelpie's guard or `confine`) | 232 | 149 |
| `path-refused` (a permission rule, the sandbox or `confine`) | 106 | 32 |
| `hook-block` (a hook not kelpie's) | 86 | 0 |
| `permission-refused` (a command outside the call's allow list) | 52 | 0 |
| `tool-not-given` (a tool name the call doesn't have, such as `bash` for `Bash`) | 49 | 0 |
| `timeout` | 22 | 8 |
| `git-ref-lock` | 16 | 21 |
| `tmp-refused` | 14 | 35 |
| `edit-miss` (an edit whose old text is not in the file, or is there twice) | 5 | 158 |
| `command-not-found` | 4 | 23 |
| `unknown-skill` | 0 (98 before 10-12) | 0 |
| `other` (a command that ran and failed) | 220 | 340 |

- No refused socket bind or network call shows as a failed result in that time. The one match for each harness was a docs table that names the error. Workers skip the tests `docs/agents/sandbox-red-tests.md` lists.
- pi's `grep` and `find` tools failed 22 times with `ripgrep (rg) is not available and could not be downloaded` and once with the same for `fd`. Neither was on the maintainer's `PATH` in that time, and an interactive Claude Code session has `rg` only through its own bundled copy. Homebrew installed `ripgrep` 15.2.0 and `fd` 10.5.0 there on 2026-10-16, so both are on the `PATH` now.
- Those two tools run `rg` and `fd`. pi looks for each in its own agent folder's `bin` first, then on `PATH` (`fd` under either name `fd` or `fdfind`), then downloads it from GitHub (`dist/utils/tools-manager.js`, pi 1.1.0). With `PI_OFFLINE` set, or `--offline` passed, the download is skipped and the tool answers with the message above. Every kelpie pi call has a pi home of its own, so its `bin` is always empty. kelpie sets `PI_OFFLINE=1` in each pi call's environment, so the download is never tried, and `doctor` reports a project with an agent on pi whose `PATH` lacks either program (#972).
- `fixtures/claude-transcript-tool-errors.jsonl` and `fixtures/pi-session-tool-errors.jsonl` hold one real failed result for each kind each harness had, the shortest found, with the call that made it. Each record is trimmed to those two blocks, with long text cut at 1500 characters and the home folder written `/Users/me`. The pi guard-block record is the recorded reason with `kelpie guard: ` on the front, as the guard extension writes it since #968.