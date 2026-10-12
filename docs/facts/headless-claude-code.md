# Headless Claude Code

Measured 2026-09-24 on Claude Code 2.1.282, `--model sonnet`, one sample each. Floor means input plus cache write plus cache read on a trivial prompt.

- Baseline floor 41,012. `--setting-sources ""` gives 22,927, and also drops hooks and plugins. `--strict-mcp-config` with an empty `--mcp-config` gives 36,935. `--disable-slash-commands` gives 36,482. All three together give 26,273. `--tools ""` grows the floor to 91,744.
- `--bare` refuses subscription login. It reads only an API key.
- Two identical fresh calls share the whole prefix: the second reads 41,010 from cache. A different working directory shares only about 10k.
- The first `--resume` onto a fresh session rewrote about 29k of cache in one early probe. Seven runs in the transport series wrote only the new turn (about 5.3k), so it did not reproduce.
- `total_cost_usd` is cumulative across a resumed session. `usage` is per call. Measured again 2026-09-26 on 2.1.283: `modelUsage` is cumulative too, and the top-level `usage` sums every request inside one call. The recorded pair is in `fixtures/`.
- A call killed within its first second leaves no transcript. `--resume` onto it prints "No conversation found with session ID" and exits 1 with no JSON, and `--session-id` can take the same id again. Once a transcript exists, `--session-id` refuses the id as already in use.
- `/compact` works under `-p`. The result carries `local_command: "compact"` and `num_turns: 0`. Context went from 125,122 to 54,484 on the next turn, which wrote a fresh 42.6k cache. `/compact <instructions>` is accepted.
- A repo's plugin does not reach a kelpie worker, measured in shep-pm/shep-kelpie#569: a headless `claude -p` in a kelpie worktree lists no `rust-house-style:rust-house-style`, and with `--plugin-dir` at the plugin's folder it lists it. 46 of 73 worker retros reported "Unknown skill: rust-house-style". Slice 1 (#570) is tested against stand-ins only; the live run on this repo was not recorded, since a worker cannot start `claude` of its own.
- Plugin skills under `-p`, measured 2026-09-29 on Claude Code 2.1.285 with Haiku 4.5, one sample each:
  - A `--plugin-dir` plugin loads under `--setting-sources project`, and `-p "/<plugin>:<skill> <args>"` runs the skill even with `disable-model-invocation: true`, on a fresh session and on `--resume` alike. Multi-line arguments reach it whole.
  - An unknown skill, or a `--plugin-dir` that doesn't exist, is no error: the call succeeds and the model answers that the command "isn't available".
  - A plugin manifest's `skills` list does not load an absolute path outside the plugin's folder. A copied skill answers to both its folder's name and its `SKILL.md` name.
- Trimming unused features, measured 2026-10-01 on Claude Code 2.1.286 with Sonnet, floor as above, two samples each, identical. A reviewer's settings file (`deny` of `Agent`, `Task` and `Bash`) gives 16,093. The same file with `disableBundledSkills`, `disableWorkflows`, `disableClaudeAiConnectors` and `disableArtifact` set, and the plan-mode, cron, design and worktree tools added to `deny`, gives 9,011: 7,082 fewer on every turn. A bare tool name in `deny` removes the tool's definition, where a scoped rule only blocks the call.
- Context floor with an allow-list, measured 2026-10-10 on Sonnet 5.5 with a trivial prompt in an empty repo, `--setting-sources project` as a worker runs, counting input plus cache write plus cache read:
  - As a worker ran: 18,385.
  - The five `disable*` settings (`disableBundledSkills`, `disableWorkflows`, `disableRemoteControl`, `disableClaudeAiConnectors`, `disableArtifact`): 12,240.
  - `--tools Bash,Read,Edit,Write,Glob,Grep,Skill --strict-mcp-config`: 10,521.
  - Both: 8,265.
  - `--tools Bash,Read,Edit,Write,Glob,Grep --strict-mcp-config`: 7,478.
  - The list had also carried the account's claude.ai connectors, `Agent`, `Workflow`, `ScheduleWakeup` and the deferred tools, none of which a worker can use. At about 10k a turn, a worker of 270 turns re-read about 2.7M tokens it never needed.
  - Kelpie now starts a worker with the third row's tools (`Bash,Read,Edit,Write,Glob,Grep,Skill` with `--strict-mcp-config`) and a reviewer with `Read,Glob,Grep`.
- Two concurrent `--resume` calls on one session both succeed and run one after the other. `--fork-session` gives a new session id and a separate transcript.
- Flags missing from `--help` but present in the binary: `--max-budget-usd`, `--task-budget`, `--autocompact`, `--session-id`, `--resume-session-at`, `--channels`, `--dangerously-load-development-channels`.
- The CLI parses rate-limit data into `five_hour` and `seven_day` windows (plus `seven_day_opus` and `seven_day_sonnet`), each with a utilization and a reset time. `-p --output-format json` does not carry it, stream-json does as `rate_limit_event`, and headless `claude -p "/usage"` reads it for free.
