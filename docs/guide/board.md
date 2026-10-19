# The board and the project manager

## The board

Issues labelled `ready-for-agent` are the board. What decides whether, and when, an issue starts:

- The board skips an issue that is assigned to anyone or already has an open pull request, and one blocked by an issue that is still open. Don't assign it to yourself
- The board also skips a ready issue labelled `in-progress` that this runner holds no work item for, since an earlier worker holds it, and `status` lists it under `skipped` as `in-progress`. Take the label off to release it. A restart still resumes the runner's own items
- An issue with sub-issues is never worked itself: its sub-issues are, and shep-kelpie closes it once every sub-issue is closed
- The board keeps the rest in an order, and takes the first it can start. A newly ready issue joins after the last issue of its priority, `priority: P0` to `P4` then none, and the oldest goes first when several join at once. An issue that closes or loses `ready-for-agent` leaves the order. With no label changes, that is the rule of priority then age, and `status` lists the first five under `order`. Changing the priority label of an issue already in the order moves it to just after the last issue of its new priority. An `agent:<name>` label, such as `agent:opus-high`, picks the agent that works it from those the project lists in `agents.implementers` (see [Agents](agents.md#agents)), and a label naming one it does not list keeps the issue off the board. With a `!` at its end, as `agent:opus-high!`, it also pins the work item to that agent, which never falls back. An issue with no `agent:` label goes to the issue writer first when the project lists more than one implementer (see [Writing issues](issues.md#writing-issues)). An old `worker:` label is no longer read: that issue counts as unlabelled, and the runner's log says so as its work item opens
- You order that list. `shep kelpie board` prints it numbered, each issue with its title, its priority, and `blocked` or `held` when the board cannot take it yet. `board move <issue> <position>` puts an issue at that place, counting from 1, and `board top <issue>` puts it first; the issues between shift by one, and the runner takes the new order from its next pick, restarts included. Only a ready issue is in the list. A move of any other, or to a position past the last or before the first, is refused and changes nothing. `board` lists what the runner last read, so it is empty until the first read of the board. With `--json` each prints the runner's object, the list under `board`
- With a project manager set up (see [The project manager](#the-project-manager)), it picks among two or more issues the board could start, and may hold some back. Without one, or while it is down, the order above picks
- A slot under `concurrency.active_items` bounds model calls. A work item takes one when it opens and keeps it through CI and the merge, but one parked on a ruling gives it up, so the board opens the next issue while it waits. Once `concurrency.pending_rulings` work items are parked, the board opens nothing new, and the alert for the ruling that filled it says so. It is the point where the board stops, not a hard limit: items already working can still park past it, and `add` ignores it. `concurrency.pending_rulings = 0` stops the board while any ruling waits, and with none waiting the board opens work as ever. A merged pull request parked on its follow-up ruling counts toward neither cap. An issue whose body names a file the branch of a parked item not yet merged changes waits for that item, and `status` lists it under `skipped` as `overlap`. One whose paths the board has not read yet, or every one while such a branch's files are not known, waits a pass and is listed as `paths-unread`
- Once you answer its ruling, a work item goes on without a slot through anything that calls no model: CI, a merge, or its end. When it needs a worker's turn or a review again, such as for your `no <note>` or a red CI run, it waits for the next slot to free, ahead of any new issue. `status` lists the open items' issues under `working`, `waiting_for_slot` and `parked`
- No turn starts while Claude's 5-hour window is at 50% or more, and no new work item starts once today's share of the week is spent. `shep kelpie status` says why under `pacer`, and `enabled = false` in the project's `[app.dogs.kelpie.pacing]` turns both off

`shep kelpie add <ticket>` opens a work item for a ticket at once, without the label, ahead of the board's order. It queues nothing: while every slot under `concurrency.active_items` is held or waited for, `add <ticket>` is refused.

On a pull request kelpie opened, `ready-for-agent` or a review requesting changes starts a rework of it, the same as `shep kelpie rework <pr>`. On any other open pull request of kelpie's account, `ready-for-agent` adopts it, the same as `shep kelpie adopt <pr>`. Adoptions and reworks go before any issue. An adoption is refused while a worktree kelpie did not make has the pull request's branch checked out, and `adopt` names that worktree. One that fails after it was queued raises an `adopt-failed` ruling, and `shep kelpie status` lists it with its error and since when. Kelpie puts `ready-for-human` on each pull request it hands back.

The runner reads the board from GitHub at most once a minute, however often something wakes it, and at once after something that changes what the board would see, such as a work item ending, a ruling answered, an `add` or an `adopt`. `status` and `drain` answer without making it read anything, from a copy the loop posts along the pass, so `upgrade` and `pause` can ask them twice a second while they wait and are never held up by a GitHub call that waits its turn behind the login's pace. A work item's step or a board read that fails is tried again after 15 seconds, then twice as long each time up to 10 minutes. When GitHub says the account's rate limit is used up, the runner makes no GitHub call until the limit resets, says so once in its log, and `status` shows when under `forge_held_until`.

## The briefing

The runner writes the board out as `board.md` in the project's folder, and `status` names the file under `board`. It is the briefing the project manager's agent reads, so that agent never runs git or gh itself, and you can read it too. Code writes it, with no model call, at start and whenever the board changes: a call starting or ending, a phase change, a ruling raised or answered, a read of the ready queue. It is written again at least once a minute while the runner looks at the board, and each write replaces the file whole. Text that names this machine is withheld from it, as from the forge. It holds:

- each open work item: its issue, phase, pull request, age and agent, the worker's last closing message, and the files its branch touches
- what each item's session is doing. A worker's turn shows when it last made a tool call or wrote output, read from its transcript (Codex's output file, pi's session file), and reads as idle after 10 minutes of neither
- the rulings waiting on you
- the ready queue in the board's order (the order the board keeps, first taken first), with each issue's priority, why the board passes over it if it does, the paths its body names in backticks, and the first 600 characters of its body, quoted
- the board's events since the project manager last read it, or the last 20 before it ever has
- the overlap: for each pair of open branches, the files both touch and the files `git merge-tree` finds in conflict, and each ready issue's named paths against them. The conflict check needs git 2.38 or later

## The project manager

A project can name an agent that manages its board, the project manager:

```toml
[app.dogs.kelpie.agents]
implementers = ["sonnet-high"]
pm = "pm"
```

Kelpie's own `pm` is Opus 5.5 at medium. Code wakes it, never a timer, when:

- a slot is free and two or more ready issues could fill it
- the branches of two open work items conflict, as `git merge-tree` finds them
- a work item is stuck: its turn failed, it stopped twice with no pull request, CI stayed red with no fix pushed (each of which already asks you a ruling), or its worker has been idle for 10 minutes
- you tell it something: `shep kelpie tell "hold #12 until the release"`

It does not wake again for an item it answered `none` for while that item is in the same state: while it is parked on the ruling waiting on you, and for an idle worker for 30 minutes after it says so, then an hour, then two hours at most. When that time ends the item is put to it again even if the worker has shown nothing since, so a hung worker is not left unattended. A new turn, a new ruling or a new phase wakes it at once, and so does a `retry`, a `re-scope` or an `ask` on the item.



Each wake carries every reason gathered since the last one, in one call that runs as a lamb like any other and that the runner never waits on, one at a time per project. It runs in `<project>/pm`, which holds `board.md` as it stood when it woke, each open work item's diff from `main` as `diffs/<issue>.diff` (cut at 64 KiB), the end of the failed run's log as `ci/<issue>.log` for an item stuck on red CI, and its notes: `pm-notes.md`, the recent log it appends to, `pm-archive.md`, the older part kelpie moves there when the log passes 32 KiB, and `pm-standing.md`, the standing notes it writes from your tells and from the project manager's `standing` answer. Of kelpie's and the shepherd's homes it reads that folder and the project's worktrees alone, and it reads your checkout, all read only, so it judges a merge or a stuck item on the code. It never reads `gh`'s token, the credentials no worker reads, or a secret-shaped file in the checkout or a worktree: the checkout's `.git/config`, and any `.env`, `.env.*`, `*.pem`, `*.key`, `.npmrc` or `.netrc`. It reaches no host but the model's, has only Read, Glob, Grep, Edit and Write and no MCP server but the repo's own (see [MCP servers](building.md#mcp-servers)), runs no command, git or gh, and may only add to the end of its notes, at most 64 KiB a write, which a hook holds it to. It answers in one JSON object: a pick, the ready issues to hold until an open work item closes, an unstick (`retry`, `re-scope`, `ask` or `none`), `standing` (the whole of its standing notes from then on, at most 4 KiB) and a reply to you.
n

Kelpie checks the answer against the board before it acts. A pick or a hold must name a ready issue the board shows, and an unstick a work item that is stuck now. Anything else is dropped and logged as `pm-answered`, with why. A `retry` answers the item's ruling yes (for CI still red, sends the worker back to it), or ends an idle worker's call and resumes its session. A `re-scope` or `ask` adds the project manager's words to the item's ruling, on one line after `PM says:`, and posts it to you again. A pick of none starts nothing until its next wake; with no work item open it lasts 30 minutes before the board's rule picks, and a hold over every issue the board could start goes at once, so a project with nothing in flight never stalls. Its reply shows in `status` under `pm.reply` and in the log.

It keeps one session per project, resumed on each wake. Once a wake leaves its context past 100k tokens, the session is compacted before the next wake. A session that cannot be resumed starts again from the board. `shep kelpie pm` opens that session in your terminal, in its settings and sandbox, as `attach` opens a worker's: the runner holds the project manager for that command and for the session it starts, each known by its pid and start time, so no wake starts until both have ended, and a wake already in flight runs to its end first. One terminal holds it at a time. When there is no session yet, `pm` starts the one its wakes then resume.

With no project manager named, while it is down (a failed call passes it over for 10 minutes), over Claude's 5-hour pace, held in your terminal, or when its pick is dropped, the board's rule picks and a stuck item waits on its ruling, as without one.

Measured on 33 real decision points from this repo and shep, a project manager briefed this way avoided 9 to 12 of 11 to 15 avoidable conflicts where the board's rule avoided 4 of 15, and decided as well as one that ran git and gh itself at about half the cost a wake. On a scripted day of 40 wakes, one session compacted as it went cost $3.31 a day on Opus, against $8.16 for a fresh session every wake, and kept what it knew.
