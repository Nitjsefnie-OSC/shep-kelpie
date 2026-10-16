# Triage

Kelpie can look at the issues other people open. It finds the ones nobody has touched, researches each against the repo, comments what it found and leaves it `needs-triage` for you.

```toml
[app.dogs.kelpie.triage]
pick = "maintainer"
```

- `pick` says whose issues. `"off"`, the default, is no one's. `"maintainer"` is the ones you open (`git.maintainer`, else the login `gh` acts as, compared ignoring case), and `"all"` is everyone's. Any other word stops the runner
- `wait_days` (default 7) is how many whole days kelpie will wait on an answer, and `max_questions` (1 to 10, default 10) is the most questions it may ask about one issue. `wait_days` does nothing yet, and `max_questions` cuts the questions the first comment asks
- `agent` (default `triager`) names the agent file that researches an issue. Its role must be `triager`. `add` writes `triager.md`, and you can edit it like any agent file
- Triage speaks as the App, so a `pick` other than `"off"` needs the GitHub App on the repo. The runner refuses to start without it and names `triage.pick`. A missing or wrong-role `triage.agent` or `agents.representative` is refused the same way
- Only an issue opened after you turned `pick` on is looked at, so an old backlog is left alone. Kelpie keeps that moment (`triage.since`) in the project's state, forgets it when you start with `pick = "off"`, and takes a new one when you turn it on again
- Once a minute the runner lists the open issues. An issue is untouched when it carries none of `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human` and `wontfix`, a bot did not open it (the App included), and it was opened after the moment above
- `shep kelpie status` shows the untouched ones, oldest first, as `triage due`. The JSON has them under `triage.due`, and leaves the key out when none is due. A listing that fails changes nothing and costs no poll
- The first pass runs when the board is open and a slot is free, after a contribution call and before the board's pick, one issue at a time. The triager reads a clean view of `origin/main` with no shell and no plugins, and has 15 minutes
- It comments once as the App, in the representative's voice, with the files, tests, related issues, draft criteria and any questions only the author can answer. A marker under the comment lets kelpie find it again. Then it puts the type (`bug` or `enhancement`) and priority (`P1` to `P4`) on the issue if the repo has those labels, then `needs-triage`, made if missing. A `P0` is never set: it comes up as a question
- Before that pass, kelpie asks whether something already covers the issue. The triager is shown the open issues and the open pull requests, each pull request with the issues its body says it closes, and answers `addressed_by`: nothing, or the one open issue or pull request that does the same thing. When kelpie can check that the number is open and is not the issue itself, it posts one short comment naming it as `#n` with the reason, closes the issue as a duplicate, and puts no labels on it. A number it cannot check is dropped, told in the log, and the issue is triaged as any other
- An issue kelpie closed as a duplicate is not triaged again on its own if the author reopens it: its record keeps it out of the due list. `shep kelpie triage <n>` triages it again
- A call or a post that fails is told once in the log and tried at the next poll. Five failures give the issue up until a restart. A restart goes on from the step an issue got to and repeats no step it finished. Only `shep kelpie triage` posts a second comment on an issue. An issue that already holds the App's marker comment is recorded with no call
- `shep kelpie triage <issue>` triages that issue next, past the `since` and author tests. It is refused when `pick` is `off`, when the issue is not open or already carries a triage label, or while its call is in flight. An issue kelpie already triaged is triaged again: a new comment is posted and replaces the record
- `shep kelpie status` shows the call in flight and each issue kelpie triaged as `triage` lines, naming an issue it closed as a duplicate and what it duplicates. The JSON has them under `triage.running` and `triage.issues`, a closed one with its `duplicate_of`
- A change to the table waits for the next start, as `contributions` does

Reading an issue is the triager's work and reaches the repo read-only on every harness: Claude Code, pi, Codex and goose. Its calls total on a `triager` line in `shep kelpie usage`, apart from the work items.
