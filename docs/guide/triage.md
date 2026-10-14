# Triage

Kelpie can look at the issues other people open. Today it only finds the ones nobody has touched and lists them in `status`. It posts nothing and labels nothing.

```toml
[app.dogs.kelpie.triage]
pick = "maintainer"
```

- `pick` says whose issues. `"off"`, the default, is no one's. `"maintainer"` is the ones you open (`git.maintainer`, else the login `gh` acts as, compared ignoring case), and `"all"` is everyone's. Any other word stops the runner
- `wait_days` (default 7) is how many whole days kelpie will wait on an answer, and `max_questions` (1 to 10, default 10) is the most questions it may ask about one issue. Neither does anything yet
- `agent` (default `triager`) names the agent file that will research an issue. Its role must be `triager`. `add` writes `triager.md`, and you can edit it like any agent file
- Triage speaks as the App, so a `pick` other than `"off"` needs the GitHub App on the repo. The runner refuses to start without it and names `triage.pick`. A missing or wrong-role `triage.agent` or `agents.representative` is refused the same way
- Only an issue opened after you turned `pick` on is looked at, so an old backlog is left alone. Kelpie keeps that moment (`triage.since`) in the project's state, forgets it when you start with `pick = "off"`, and takes a new one when you turn it on again
- Once a minute the runner lists the open issues. An issue is untouched when it carries none of `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human` and `wontfix`, a bot did not open it (the App included), and it was opened after the moment above
- `shep kelpie status` shows the untouched ones, oldest first, as `triage due`. The JSON has them under `triage.due`, and leaves the key out when none is due. A listing that fails changes nothing and costs no poll
- A change to the table waits for the next start, as `contributions` does

Reading an issue is the triager's work and reaches the repo read-only on every harness: Claude Code, pi, Codex and goose. Its calls total on a `triager` line in `shep kelpie usage`, apart from the work items.
