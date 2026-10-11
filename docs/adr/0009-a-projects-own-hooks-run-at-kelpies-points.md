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

### Payload and exit

The command gets one JSON object on stdin and runs in the worktree, or in the project's checkout for a post of kelpie's own. Every payload has `point`, `project`, `role` (`worker`, `issue-writer` or `kelpie`) and `repo` (`owner/name`). The rest depends on the point:

- `publish`: `field` (what the text is, such as "the comment") and `text`.
- `commit`: `message` (and `messages`, one for each `-m`) and `diff`.
- `push` and `handover`: `range`, `base`, `head`, `commits` (each `sha` and `message`) and `diff`. `handover` adds `pull_request` and `branch`.

The exit is the answer:

- **0** passes.
- **2** refuses, with stderr as the reason.
- **Anything else** is a hook that failed: it refuses and says it failed. A signal and a program that is not there are the same. A check that cannot run must not pass silently.

### Where a refusal goes

- **An agent's own action** gets the refusal back as the tool's error, as `kelpie guard` does now: a `git commit`, a `git push` or a `gh` command is refused with the hook named and its reason.
- **A worker's pull request file** (`pull-request.md`, which kelpie opens the draft from) goes back to the worker as its next prompt, with the reason. A turn that is refused again parks the work item on a ruling naming the hook and its reason.
- **A refused `handover`** goes back to the worker once. A second refusal parks the work item on a ruling naming the hook and its reason, whose yes sends the worker back again.
- **A reply on a review bot thread** whose reason a hook refuses is posted with a fixed reason instead, so the threads after it are answered. A hook that failed is retried.
- **A deferred-finding issue** a hook refuses is held and retried for the refusal window, then the maintainer is asked, as for a forge that refuses.
- **A ruling's post** that a hook refuses is not posted. The ruling stays parked, and the failed post is told in the step's report. The maintainer still gets the ruling by webhook and `status`.

### The commit and push points ride on `kelpie guard`

Every harness already runs `kelpie guard` on each command, and `kelpie confine` on each write, so the hooks reach pi and Codex with no Claude-shaped hook. The guard's command line carries them as one `--hooks=<json>` argument, with the project's name and repo. A pi or Codex implementer beside `hooks` is not refused at load. The guard hook runs outside the agent's sandbox, so a hook can read config the agent cannot, and the agent can neither edit nor skip it.

### `worker.guard_hooks` stays

`worker.guard_hooks` is still the Claude Code-only layer: a `PreToolUse` or `PostToolUse` hook on a tool call of any kind, with Claude Code's matcher and payload. Use it for a check on a tool call kelpie has no point for, on Claude Code alone. Use `hooks` for a check on what is written, committed, pushed or posted, which then holds for every harness and role. A project can have both.

## Open questions, decided

- **`publish` covers the text kelpie writes from templates, not only an agent's.** A ruling's question carries a worker's words, a finding quotes the code, and a project's private word turns up in text kelpie composed as readily as in text an agent did. A hook that sees only agent text would miss those, and kelpie cannot tell them apart where it posts. A template that never carries the word costs one run of the hook.
- **A slow hook is a failed hook, and refuses.** A hook runs at most 30 seconds, then it is killed. Passing a hook that did not finish would pass what the project asked to be checked. The timeout is one constant, not a setting, until a project needs a hook that runs longer.
- **A hook only refuses.** What it prints on stdout is dropped. A redaction would post text the agent never wrote, and the agent, the reviewers and the forge would then disagree about what was said.

## Considered options

- **Run the hooks as Claude Code hooks for every harness.** Pi and Codex have no such hooks, and a role with no Bash never runs one.
- **Make `hooks` replace `worker.guard_hooks`.** A project may rely on a Claude Code hook on a tool call, which kelpie has no point for.
- **Let a hook rewrite text.** Decided against above.
- **Let a slow hook pass.** Decided against above.
- **Cover only the text an agent wrote.** Decided against above.
- **A timeout setting for each hook.** A knob nobody asked for. A constant is easy to turn into a setting.

## Consequences

- A refusal that repeats the text it refused (a hook that prints the word it found) is refused again when it is posted as a ruling. The ruling stays parked and reaches the maintainer by webhook and `status`. A hook should name the rule, not the text.
- `edit_handed_over` (a later turn that rewrote `pull-request.md`) tells a refusal in the log only, and leaves the pull request as it was.
- A hook that lists `commit` and `push` both runs on a commit and again on the push that sends it, so it should be fast.
- Neither the reviewers nor the project manager has a tool that posts: what they write reaches the forge through kelpie, where `publish` runs.
- A work item in the state file gains `handed_over` and `handover_refused`, both omitted when empty, so an older state file loads as it was.
