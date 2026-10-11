# A project's own hooks run at kelpie's points

`worker.guard_hooks` copies a project's checks into a Claude Code worker's settings file as Claude Code hooks. So they reach a Claude Code worker and nothing else:

- A pi or Codex implementer beside them is refused at load.
- Reviewers, the project manager and the issue writer never run them.
- Kelpie's own posts (rulings, review thread replies, deferred-finding issues) never run them.

When `private_names` was removed (shep-pm/shep-kelpie#380), the maintainer accepted that last gap. Shep-pm/shep-kelpie#436 closes it: kelpie runs the project's hooks itself, at fixed points, whatever the harness or role.

## Decision

A project setting, `hooks`, is a list of `{ point, command }`. Kelpie runs `command` under `sh -c` at its `point`. Every command must resolve when the runner starts, as a `worker.guard_hooks` command must, or the runner stops and names it.

### Points

- **`publish`**: before any text goes to the forge, from any role or from kelpie itself. That is a pull request's title and body, a comment, a reply on a review thread, an issue's title and body, and a ruling's post. The forge and voice ports run it after the local-path check, so one port serves a post through `gh` and through the App. A worker's own `gh pr create`, `gh pr edit` and `gh issue create` run it in `kelpie guard`, and the issue writer's `gh issue create` in its guard.
- **`commit`**: before a worker's commit lands, with its message and the staged diff (every tracked change under `-a`). `kelpie guard` runs it on a `git commit` in the worker's worktree.
- **`push`**: before a worker pushes, with the commits being pushed past `origin`'s base branch. `kelpie guard` runs it on a `git push`. A commit written and committed in one call is not staged when `commit` runs, and `push` is what sees it.
- **`handover`**: when a worker ends a turn with the branch pushed and a pull request known, before a reviewer reads it. It runs once for each head.

shep-pm/shep-kelpie#454 added nine more, to cover the events Claude Code and Codex offer, so a project that wants one has it on every harness. A point is a gate, which refuses as the four above do, or a notice, which only tells.

Gates on an agent's tool call, run by `kelpie guard` and `kelpie confine`:

- **`command`**: before any shell command an agent runs, the worker's and the issue writer's. This is Claude Code's and Codex's `PreToolUse` on a shell command, and the point a check on every command, such as a git gate, belongs at.
- **`write`**: before a file write or edit. It is `PreToolUse` on a file edit (`apply_patch` in Codex), run by `kelpie confine` after it has held the write to the worker's folders, once for each file the call writes.
- **`after_command`**: after a shell command has run, failed or not. It is `PostToolUse` and `PostToolUseFailure`. It cannot undo the command, so a refusal reaches the agent as feedback on the result.

Gates in kelpie's own work item loop, run in the runner, outside the sandbox:

- **`turn_start`**: before kelpie sends a worker, a reviewer or the project manager a prompt. This is `SessionStart` and `UserPromptSubmit`.
- **`turn_end`**: when a worker's turn ends, every turn. This is `Stop` and `SubagentStop`; `handover` still covers a handed-over turn.

Notices in the same loop, which tell and never refuse:

- **`reset`**: before kelpie resets a worker or the project manager. This is `PreCompact`.
- **`ruling`**: when a ruling is raised. This is `Notification`, and lets a project send rulings anywhere without a webhook kind for each service.
- **`ci`**: when CI settles on a head.
- **`done`**: when a work item ends, merged or not. This is `SessionEnd`.

Left out on purpose: the events about a person at the terminal (`Interrupt`, `Elicitation`, `MessageDisplay`, `ConfigChange`, model switches), the ones kelpie owns itself (`WorktreeCreate`, `WorktreeRemove`, `Setup`), Claude Code's task and teammate events, which kelpie's workers do not use, and `PermissionRequest`, since a worker never asks the user for permission and a question is a ruling.

### Payload and exit

The command gets one JSON object on stdin and runs in the worktree, or in the project's checkout for a post of kelpie's own. Every payload has `point`, `project`, `role` (`worker`, `issue-writer` or `kelpie`) and `repo` (`owner/name`). The rest depends on the point:

- `publish`: `field` (what the text is, such as "the comment") and `text`.
- `commit`: `message` (and `messages`, one for each `-m`) and `diff`.
- `push` and `handover`: `range`, `base`, `head`, `commits` (each `sha` and `message`) and `diff`. `handover` adds `pull_request` and `branch`.
- `command`: `command` (the full line) and `cwd`.
- `write`: `path` (from the worktree's root), `tool`, and `content` (a write's text or an edit's new text) or `patch` (the text of a Codex `apply_patch`, which names each file it writes; the hook runs once for each).
- `after_command`: `command`, `cwd`, `exit` and `output` (the last 4000 characters of what it printed). `exit` is read from the harness's result in whichever shape it gives, and is `null` when a failure names none.
- `turn_start`: `issue` (`null` for the project manager), `phase`, `session` (`new` or `resumed`), `recipient` (`worker`, `reviewer` or `pm`) and `prompt`. `role` is `worker` for a worker's prompt and `kelpie` for the others.
- `turn_end`: `issue`, `phase`, `last_message` and `head` (`null` before the branch is pushed).
- `reset`: `issue` (`null` for the project manager), `phase`, `recipient`, `session`, `kind` (`compact`) and `handoff`. A reset today is a `/compact`, which carries no handoff of kelpie's (the model writes its own summary), so `handoff` is `null`; it will carry the text when a step drives a clear with a handoff.
- `ruling`: `id`, `issue`, `pull_request`, `kind` (`merge`, `question`, `stuck`, `agent-files`, `foreign-change` or `follow-up`), `question` (the text the maintainer reads) and `choices` (`yes`, `no` and for a merge `rework`, none for a question, which takes free text).
- `ci`: `issue`, `pull_request`, `head`, `conclusion` (`passed`, `failed` or `skipped`) and `failed` (the names of the checks that failed).
- `done`: `issue`, `pull_request`, `merged`, `closed` and `history`, the entry `status` shows for it.

The exit is the answer, at a gate:

- **0** passes.
- **2** refuses, with stderr as the reason.
- **Anything else** is a hook that failed: it refuses and says it failed. A signal and a program that is not there are the same. A check that cannot run must not pass silently.

At a notice nothing refuses. A hook that exits non-zero, or hangs past the 30 seconds and is killed, is logged with its name, in the runner's notes, and kelpie goes on. Every hook at the point still runs.

### A git gate as a `command` hook

The maintainer's own git gate checks every shell command, so it could not move to `hooks` while only commits, pushes and posts had a point, and was dropped from a project that runs pi. As a `command` hook it holds on every harness:

```toml
[[app.dogs.kelpie.hooks]]
point = "command"
command = "~/bin/git-gate"
```

```sh
#!/bin/sh
# Reads the payload's `command`, and refuses what a worker must not run.
line=$(jq -r .command)
case "$line" in
  *"git reset --hard"* | *"git push --force"* | *"git push -f"*)
    echo "the git gate: leave history alone" >&2
    exit 2
    ;;
esac
```

### Where a refusal goes

- **An agent's own action** gets the refusal back as the tool's error, as `kelpie guard` does now: a `git commit`, a `git push` or a `gh` command is refused with the hook named and its reason.
- **A worker's pull request file** (`pull-request.md`, which kelpie opens the draft from) goes back to the worker as its next prompt, with the reason. A turn that is refused again parks the work item on a ruling naming the hook and its reason.
- **A refused `handover`** goes back to the worker once. A second refusal parks the work item on a ruling naming the hook and its reason, whose yes sends the worker back again.
- **A refused `turn_end`** goes back the same way: once with the reason, then a ruling naming the hook. It runs before `handover` on a turn that ends with no question and no work left uncommitted, and a turn it refuses is not handed over. A turn that ends on a question block is not judged, since sending the worker back would answer for the maintainer.
- **A refused `command` or `write`** is the tool's error, as a guard refusal is.
- **A refused `after_command`** reaches the agent as feedback on the command's result: Claude Code's and Codex's `PostToolUse` hook (and Claude Code's `PostToolUseFailure`, which it fires for a command that failed) exits 2 with the reason on stderr, and pi's `tool_result` handler adds the reason to the result and marks it an error. The command has run by then.
- **A refused `turn_start`** parks a worker on a ruling naming the hook, since the call that would have run fails like one that could not start. The prompt is not sent. A reviewer's refused prompt is a failed round, retried and then skipped as any failed round is, and the project manager's is a failed wake.
- **A reply on a review bot thread** whose reason a hook refuses is posted with a fixed reason instead, so the threads after it are answered. A hook that failed is retried.
- **A deferred-finding issue** a hook refuses is held and retried for the refusal window, then the maintainer is asked, as for a forge that refuses.
- **A ruling's post** that a hook refuses is not posted. The ruling stays parked, and the failed post is told in the step's report. The maintainer still gets the ruling by webhook and `status`.

### The commit and push points ride on `kelpie guard`

Every harness already runs `kelpie guard` on each command, and `kelpie confine` on each write, so the hooks reach pi and Codex with no Claude-shaped hook. The guard's command line carries them as one `--hooks=<json>` argument, with the project's name and repo. A pi or Codex implementer beside `hooks` is not refused at load. The agent can neither edit nor skip a hook, since the guard's command line is pinned in the call's settings. Where a hook runs differs by point. `kelpie guard` runs inside kelpie's OS sandbox, in the worker's fence, so a `commit`, `push` or worker `publish` hook is a child of the guard: it sees the worker's environment and can read only what the worker can, and a hook that needs config the fence hides fails and so refuses every such command. `command` rides on the same guard, `after_command` on a second entry of it (`kelpie guard --after`, registered as `PostToolUse` and `PostToolUseFailure` for Claude Code, `PostToolUse` for Codex, and pi's `tool_result` event in the guard extension), and `write` on `kelpie confine`, which carries the same `--hooks=<json>` argument. Only `handover`, `turn_start`, `turn_end`, the four notices and kelpie's own `publish` run in the runner, outside the sandbox, with the runner's access. A hook that needs a secret belongs at one of those points, or reads it from a path the fence leaves readable.

### `worker.guard_hooks` stays

`worker.guard_hooks` is still the Claude Code-only layer: a `PreToolUse` or `PostToolUse` hook on a tool call of any kind, with Claude Code's matcher and payload. Use it for a check on a tool call kelpie has no point for, on Claude Code alone. Use `hooks` for a check on what is written, committed, pushed, posted or run in a shell, which then holds for every harness and role. A project can have both. The load-time refusal of a pi or Codex implementer beside `worker.guard_hooks` says `command` or `write` is the harness-neutral way.

## Open questions, decided

- **`publish` covers the text kelpie writes from templates, not only an agent's.** A ruling's question carries a worker's words, a finding quotes the code, and a project's private word turns up in text kelpie composed as readily as in text an agent did. A hook that sees only agent text would miss those, and kelpie cannot tell them apart where it posts. A template that never carries the word costs one run of the hook.
- **A slow hook is a failed hook, and refuses.** A hook runs at most 30 seconds, then it is killed. Passing a hook that did not finish would pass what the project asked to be checked. The timeout is one constant, not a setting, until a project needs a hook that runs longer.
- **A hook only refuses.** What it prints on stdout is dropped. A redaction would post text the agent never wrote, and the agent, the reviewers and the forge would then disagree about what was said.

Decided on shep-pm/shep-kelpie#454, for the points that cover the CLIs' events:

- **No hook rewrites anything and stdout stays dropped.** That rules out the CLIs' `additionalContext` and `updatedInput`. A project that wants to add context does it in its CLAUDE.md or skills, which every harness reads.
- **`PermissionRequest` has no point.** Kelpie's workers never ask the user for permission; a question is a ruling, which `ruling` covers.
- **A notice gets the same 30 second timeout.** A notice that hangs is killed and logged, and holds nothing up. A notice cannot refuse, so a non-zero exit is a log line with the hook's name and nothing more.
- **`worker.guard_hooks` stays** as the Claude Code-only layer for a tool kelpie has no point for. The load-time refusal says `command` or `write` is the harness-neutral way.
- **`reset` fires before a `/compact`, the one reset a step drives.** A clear with a handoff is driven by no step yet, so `handoff` is `null` until one is. The hook runs on the thread of the call that sends the `/compact`, so a slow one delays only that call.
- **`turn_start` runs on the thread of the call, not in the loop.** The agent port is wrapped once for each call, so a hook holds up that call and nothing else, and every role's prompt passes the one place. A local review round (a script, not a prompt to a session) is not a prompt and is not judged.
- **`ruling` fires once for each ruling, in the pass that raised it, whether or not a webhook is set.** What a restart finds is taken as told, so a restart does not tell a ruling twice, and does not tell one raised while the runner was down. `ci` fires once for each head a work item's CI settles on, and a restart may tell the head again.
- **`turn_end` does not judge a turn that ends on a question.** The worker is waiting on the maintainer, and sending it back would answer for them.
- **A hook reads the harness's own result for `after_command`.** Claude Code's and Codex's `PostToolUse` payload shapes are read leniently (`tool_response` as an object with `stdout`, `stderr` and an exit, or as a text; a failure event's `error`, which Claude Code starts with `Exit code N`). Neither has been checked against a live session.

## Considered options

- **Run the hooks as Claude Code hooks for every harness.** Pi and Codex have no such hooks, and a role with no Bash never runs one.
- **Make `hooks` replace `worker.guard_hooks`.** A project may rely on a Claude Code hook on a tool call, which kelpie has no point for.
- **Let a hook rewrite text.** Decided against above.
- **Let a slow hook pass.** Decided against above.
- **Cover only the text an agent wrote.** Decided against above.
- **A timeout setting for each hook.** A knob nobody asked for. A constant is easy to turn into a setting.
- **One point for each CLI event.** The CLIs' 33 and 12 events mostly cover a person at a terminal or what kelpie owns. Nine points cover the events a worker meets, and the table of what each covers is above.
- **Run `turn_end` and the other loop points as Claude Code hooks.** Pi and Codex have no `Stop`, and a reviewer has no Bash.

## Consequences

- A refusal that repeats the text it refused (a hook that prints the word it found) is refused again when it is posted as a ruling. The ruling stays parked and reaches the maintainer by webhook and `status`. A hook should name the rule, not the text.
- `edit_handed_over` (a later turn that rewrote `pull-request.md`) tells a refusal in the log only, and leaves the pull request as it was.
- A hook that lists `commit` and `push` both runs on a commit and again on the push that sends it, so it should be fast.
- Neither the reviewers nor the project manager has a tool that posts: what they write reaches the forge through kelpie, where `publish` runs.
- A work item in the state file gains `handed_over` and `handover_refused`, both omitted when empty, so an older state file loads as it was. The new points add no state: what they have told is kept in memory.
- A hook at `command` runs on every shell command, so it should be fast, and it runs again on a Codex command that runs in another folder of the worktree, which the pinned command judges again.
- The point names are `snake_case` in the settings, which changes nothing for the four single words that were there first.
