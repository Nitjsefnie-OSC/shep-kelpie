# The panel

Four experts vote on each ruling that lists two options or more, each blind to the others, so a ruling reaches you with their reasons beside it:

| Expert | Who | Reads |
|---|---|---|
| Worker | the work item's own session, resumed once with the ruling | what it already holds, and its worktree. It abstains when the work item has no worker turn |
| PM | the project manager, woken as for anything else | the board, the diffs and the CI logs |
| Advisor | the `advisor` agent file | the codebase: the work item's worktree and your checkout, its diff, and the design docs, ADRs, glossary and README |
| Researcher | the `researcher` agent file | everything outside the codebase: earlier rulings and their answers, the issue and pull request threads, the worker's session, your own agent sessions, and the web |

A project has a panel once it names an advisor or a researcher:

```toml
[app.dogs.kelpie.agents]
advisor = "advisor"
researcher = "researcher"
```

Kelpie's own `advisor` is Opus 5.5 at high and its `researcher` Sonnet 5.5 at high, and either runs on any harness kelpie runs. Neither runs git, `gh` or a command, and neither writes anything: its reply is its vote. Kelpie writes what each reads into a folder of its own before it calls it: `<project>/advisor` holds `ruling.md`, `diff.patch` (the branch's diff from `main`, cut at 64 KiB) and `where.md`, and the advisor reads your checkout and the project's worktrees too, under the project manager's fence, with no secret-shaped file. `<project>/researcher` holds `ruling.md`, `rulings.md` (the latest 60 rulings answered, and how, from the file `rulings.jsonl` that kelpie adds to as each is answered, so it holds the rulings answered from this build on), `threads/` (the issue and the pull request, with their latest comments), `worker.md` (the worker's session) and `maintainer/` (your sessions, below). Its file tools read that folder and nothing else. It has web search and fetch, read only; a page it reads can try to steer it, so its vote is one of four and never decides alone. Its fetch reaches the hosts `researcher.allowed_domains` names, and its search needs none.

`researcher.transcripts` names folders of your own agent sessions, read only, and is off until you name one. A folder may be a harness's own place, `~/.claude/projects` for Claude Code, `~/.codex/sessions` for Codex or `~/.pi/agent/sessions` for pi: kelpie reads a session only if it ran in the project's checkout or under it, takes the latest twelve, and keeps only what was asked and said, the last 96 KiB of each and 512 KiB in all.

```toml
[app.dogs.kelpie.researcher]
transcripts = ["~/.claude/projects"]
allowed_domains = ["docs.rs"]
```

Each vote is one JSON object, `{"choice": "<option key>", "why": "<at most 350 characters>", "design": true|false}`, and `design` is true when the question changes an ADR, the glossary, a public interface or the project's direction. A reply with no such object, or a choice that is no option, is an abstention, and the log says why (`ruling 1: the advisor abstains, since ...`). The experts run at once, and the ruling's comment waits for their votes for 20 minutes at most from when they are asked, then lists the Panel as it stands:

```markdown
**Panel**
- **Worker** (A): Both threads were fixed in 6b0606e with tests.
- **PM** (A): CI green, every reviewer read it.
- **Researcher** (A): Ruling 125 settled the one open design point.
- **Advisor** (B): The hook skips a refused write, and no test covers it.
```

The votes are kept with the ruling, and what they decide is below.

## Deciding

Three experts on one option decide it, unless one flagged `design`. Anything less is a split, and you are the tie-breaker. You get a split, a merge on a project under `git.merging = "ask"` and any `design` flag, each with every vote, and the last two with the panel's recommendation:

```markdown
**Recommendation: A** (3 of 4)
```

Everything else is applied at once and posted on the ruling's thread headed `Decided by the panel`, with the votes beneath. A reply there of `<id> no <note>` reopens it: the ruling comes back for its work item and your note is the answer, so the worker takes it as it takes any `no`. It needs the work item to be open and its worker not running, and says why when it is not; a decision is watched for 24 hours.

`rulings.decide` sets whether the panel decides:

```toml
[app.dogs.kelpie.rulings]
decide = "shadow"
```

- `shadow`, when absent: the panel votes and posts on every ruling and you still answer. Every expert votes on every ruling under it. `status` reports a `panel` with the project's current run of rulings where the panel's recommendation was your answer (`streak`). A ruling the panel had no recommendation for, a split, leaves the run as it is, and an answer that differs, or is no option, ends it. Switch to `panel` when the run is long enough.
- `panel`: the panel decides as above.

Only your own answers count: the project manager's and the panel's are left out of the run.

## Who votes

Not every ruling needs four votes. The work item's type, from its pull request's conventional title, sets the panel under `panel`:

| Type | Asked |
|---|---|
| `docs` | the project manager |
| `fix`, `refactor`, `perf`, `test`, `ci`, `chore`, `style` | the project manager and the advisor |
| `feat`, a type marked breaking with `!`, or no type | the project manager, the advisor and the researcher, with the worker: all four |

A smaller panel decides only when everyone it asked picks the same option and nobody flags `design`. Otherwise kelpie asks the next panel up, or the full panel at once when a vote flags `design`, and the full panel decides on three votes of four. A vote the experts of the smaller panel already gave is not asked again. A merge under `git.merging = "ask"` goes to you whatever the panel says. A pull request whose title cannot be read is all four's.

The type is what the worker wrote in the title, so two rules only ever raise the panel, whatever the type says:

- A `question` ruling is all four's.
- A ruling on a work item whose diff from its merge base touches a path on `rulings.full_panel_paths` is all four's. The list is globs and is empty when absent, which changes nothing. A diff kelpie cannot read counts as touching one.

```toml
[app.dogs.kelpie.rulings]
full_panel_paths = ["src/guard/**", "src/github/**", ".github/workflows/**", "src/settings/**", "src/state.rs"]
```

The list above is a starting point for this repo: the guard, the forge calls, the workflows, the settings and the state file.

`rulings.pm_alone` lists kinds of ruling the project manager decides with its own vote on any type of work, for the kinds whose answer turns on facts it reads from the board. It is empty when absent:

```toml
[app.dogs.kelpie.rulings]
decide = "panel"
pm_alone = ["turn-timeout", "still-red"]
```

The kinds are `question`, `agent-files`, `foreign-change`, `follow-up`, `rebase`, `still-red`, `merge-refused`, `closed`, `fix-not-pushed`, `test-still-fails`, `unpushed`, `turn-timeout`, `turn-failed`, `unlabelled`, `adopt-failed` and `over-budget`. `merge`, `contribution-merge` and `security` are refused: the runner does not start, naming `rulings.pm_alone`, and a change that adds it while the runner runs is refused too, keeping the settings it has. A question, and a ruling on a risky path, are all four's even when their kind is listed, so `question` under `pm_alone` only fills its `pm_alone` run in `status`. A `design` flag from the project manager calls the full panel, and an abstention the panel the item's type sets, or all four for a question or a risky path. Under `shadow` every expert still votes, and `status` reports, for each kind of ruling, the run of rulings where the project manager's vote was your answer (`pm_alone`), the way to see which kinds to list. A vote that flagged `design` or abstained leaves a kind's run as it is.

An expert's agent can be off the panel: an advisor or researcher the project does not name is never asked, and counts as an abstention, so a project without a researcher decides a `feat` on three of three.

## Measuring

The panel costs up to four agent calls on every ruling, one of them Opus at high effort, so each expert has to show it is worth its cost.

- Every vote is a line in the usage ledger (`usage.jsonl`) with `votes`: the ruling's id and kind, the expert, the panel the item's type set (`pm`, `pm+advisor` or `all`), the type, and its choice, left out for an abstention. The line holds the call's tokens, cost and wall time as any call's does. The project manager's wake is counted the same way: its line has a vote for each ruling it voted on, and shares its cost between them.
- `shep kelpie usage` splits the panel's votes by expert, by kind of ruling and by type: calls, cost, median wall time, abstentions, and the cost per ruling. A wake that also served another reason counts in full.
- `status` reports, as `panel`, each expert's votes, abstentions, `agreed` (votes that were your answer) and `lone` (times it alone was right, such as the one vote for a `no` you then gave), and what each smaller panel, asked alone, would have decided against your answers (`panels`: how many, and how many as you answered). It comes from `rulings.jsonl`, which holds the rulings answered from this build on. `skipped` counts the answered rulings the panel cast no vote on, by reason: `draining` (the runner drained and no expert could be asked within the hold) or `posted` (the comment had gone out before the panel took it up). Each such ruling carries its reason as `skipped` in `rulings.jsonl`. An expert that is never the only one right adds cost and no decisions. Each expert's `agreed` and `lone` are also split by kind of ruling (`by_kind`). Both count only your answers, not the project manager's or the panel's.
- `status` also reports, as `whatif`, how seven candidate policies for who votes would have done over the rulings answered under `shadow`, where every expert votes blind so any policy can be played over the same ballots with no extra call. The candidates are fixed in code and are not a setting:
  - `type`: the table above, none of the rules after it
  - `type+risk`: the table with the rules after it, which is what `panel` does
  - `kind+risk`: all four for a question, a `design` flag or a risky path, the project manager and the advisor for every other ruling
  - `kind+risk-researcher`: the same with the researcher in the advisor's place
  - `pm+advisor` and `pm+researcher`: that pair on every ruling, with nobody above it
  - `all`: all four on every ruling

  For each: `decided` (rulings it would have decided without you), `matched` (of those, the ones it decided as you answered), `maintainer` (rulings it would have sent to you) and `cost`, per ruling, summed from the ledger lines of the votes it would have asked, an escalation's included. A call with no reported cost adds nothing, and `cost` is `null` when the usage ledger cannot be read. Only rulings answered under a build with `whatif` count, since an older line does not say whether it was a shadow run or a risky path.
- After a project's first 30 panel rulings (`facts_due` in `status`) `docs/facts/the-panel.md` gets an entry: cost per ruling and per expert, each expert's agreement and lone-correct count, abstentions, what each smaller panel would have decided against your answers, and the `whatif` table. Use it to keep, drop or downgrade an expert, move a type to a smaller or larger panel, pick the policy `panel` should use, or list more kinds under `rulings.pm_alone`.
