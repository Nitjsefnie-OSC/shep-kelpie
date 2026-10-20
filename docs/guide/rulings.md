# Rulings

A ruling is a decision only you make. The worker waiting on one is parked, and the rest of the project goes on: a parked work item gives its slot up, so the board can open another (see [The board](board.md#the-board)). It is one of six kinds:

- `merge`: CI is green, and a yes merges the pull request. A `no <note>` sends the worker your note, and its fix goes to CI and back to you with no new pass of the review; a `rework <note>` sends your note for a change the whole review reads again. On a merge ruling that warns of open review bot threads, a pass no reviewer read or a head nothing vouches for, `no` starts a new pass too, since only a pass clears that, and the question says so. The merge ruling after a `no` fix, after the fix for a review bot's late review (see [Review bots](review.md#review-bots)), or after the fix for a person's comment (see [Comments from people](review.md#comments-from-people)), says its head is that fix and that no reviewer read it, and comes back to you even under `auto`. Open review bot nits are named on their own and hold nothing. On someone else's pull request the merge ruling is always yours, `auto` included, offers yes and no only, and a `no <note>` goes to the contributor (see [Contributions](contributions.md))
- `question`: the worker asks, and your answer is its next turn
- `stuck`: the work item cannot go on by itself, and its reason says why: `rebase`, `still-red`, `merge-refused`, `closed`, `fix-not-pushed`, `test-still-fails`, `unpushed`, `turn-timeout`, `turn-failed`, `adopt-failed` or `over-budget`. The question says what a yes does. For `adopt-failed` (an adopted pull request cannot start, usually because a worktree of yours holds its branch) no work item is parked: remove that worktree or switch it off the branch, and kelpie starts the pull request on its next poll. A yes only clears the ruling, and it comes back only if the error changes. A turn that passes its ceiling while still working raises no `turn-timeout` ruling: if its worktree holds a commit that `main` lacks, made in the last 10 minutes, or an uncommitted file written in them, kelpie resumes the turn with a prompt to commit what builds and carry on, up to 3 times for a work item, and the fourth ceiling raises the ruling. For `test-still-fails` (a reviewer's test still fails after the worker's two fixes) a yes sends one more fix turn on the same tests, and a no sends the worker your note, ends the check and lets the review go on. The comment on the pull request says only that a test fails; the output tails are in the terminal question
- `agent-files`: the pull request changes agents' own files, and a yes accepts them
- `foreign-change`: someone else changed the pull request, and a yes accepts the change
- `follow-up`: a merged pull request left findings unfixed, and a yes files them as issues

A ruling shows in the runner's log, in `shep kelpie status` and in `shep kelpie rule`, and posts to the webhook when kelpie's `[kelpie.webhook]` is set (see [The webhook](#the-webhook)). The pull request gets a comment for every ruling but a merge. Answer it from the terminal:

- `shep kelpie rule 14 yes`
- `shep kelpie rule 14 no rename the flag`
- `shep kelpie rule 14 rework split the parser out`, for a merge ruling only
- `shep kelpie rule 15 use --dry-run`, for a worker's question. A `yes` there is the answer's text
- `shep kelpie rule 14 rerun`, for a `still-red` ruling that offers it: kelpie reruns the head's failed jobs once more and looks at CI again
- `shep kelpie rule 14 restore`, for a `turn-failed` ruling that offers it: kelpie puts the uncommitted agent files back as the head has them and resumes the turn

A ruling is named by its pull request's number, or by its ticket's id while it has no pull request: `rule 427 yes` answers the ruling on 427, and `rule k12 yes` one on the local ticket `k12`, which kelpie files none of yet (see [Commands](commands.md)). A work item waits on one ruling at a time, and GitHub numbers issues and pull requests from one sequence, so within a project a number names one ruling. `rule #427 yes` and `rule shep-kelpie#427 yes` read the same number, and a local id takes the `#` the same way: `rule shep-kelpie#k12 yes`. From a clone, the clone's project is looked at first. Elsewhere a number that one project has a ruling on works, and one that two projects have is refused with their names: `rule shep-kelpie#427 yes` picks one. Quotes are optional, but zsh still needs them around a note with `?`, `*`, `!` or an apostrophe. `-p` goes before the answer: after its first word, and after a `--`, a `-p` is part of the answer. `shep kelpie rule` alone lists the rulings waiting and asks which to answer and how. Each ruling's question says what `yes` and `no`, and on a merge ruling `rework`, do. A `no` or `rework` with a note is the worker's next turn.

Where an App covers the repo, a ruling's comment is one short format, whatever its kind: the problem where there is one, a title, a table of what picking each option leads to, a Panel list, and the ways to answer. The Panel lists each expert's vote once the panel has voted (see [The panel](panel.md#the-panel)), and holds the project manager's words where it gave any until then, or when the project has no panel. Here is a merge ruling. Kelpie writes the effect of each option from what it knows: the items held behind the pull request, CI, the reviewers that read it, and roughly what another review round costs from the ledger:

```markdown
### Merge this pull request at 6b0606e into main?

| | Option | If you pick it |
|---|---|---|
| **A** | Merge | Merges it into main and closes #567: the worker retro is off unless a project names it. CI passed. 2 review rounds read it. |
| **B** | Send back | Sends the worker back for a fix that goes to CI and back to you, with no new review round. |
| **C** | Rework | Sends the worker back for a change the whole review reads again. Another review round costs about $0.80. |

@octocat
- 👍 or 👎 answers, or tick one:
- [ ] A
- [ ] B
- [ ] C
- Or reply `14 no <note>` or `14 rework <note>`
```

A worker's question opens with the problem, and each option carries its effect. The worker writes both in its `<kelpie-question>` block: a first line `Problem: ` of one or two plain sentences, then each option as `- label => what picking it leads to`. The project manager rewrites the problem when it passes the question on, and adds a `board` note of what the board shows (items held behind this one, a slot freed or kept), which the card shows under the table. An option the worker gave no effect keeps the phrase its kind always had ("Sends the worker this answer"). The effect is no part of the answer the option gives.

```markdown
The push hook rejects two commits, and nothing says why.

### Which way should the push go?

| | Option | If you pick it |
|---|---|---|
| **A** | Wait | #495 stays parked until the hook is fixed. |
| **B** | Push anyway | Publishes both commits to main, and the hook stays broken. |
| **C** | Other | Reply with your own answer |

**On the board**
Nine ready issues are held behind #495.
```

You answer it in the thread, from the comment itself or a reply:

- A ruling that takes a yes or a no (`stuck`, `agent-files`, `foreign-change`, `follow-up`) takes your 👍 or 👎 on its comment. A merge ruling takes them too, as A and B.
- A merge ruling, and a worker's question that lists options, list them as task-list boxes. Tick one. A worker's question becomes the table from the options it lists, plus an `Other` row for a reply.
- A 👎, or a ticked `Send back` or `Rework`, carries no note, so the worker is told you gave none. Reply `14 no <note>` to send one.
- A tick counts only when every edit of the comment was by you or by the App that wrote it, which GraphQL's `userContentEdits` says. Anyone with write access can edit a comment, so one edit by someone else leaves the boxes dead and your 👍 or 👎 or a reply answers instead. A 👍 with a 👎 from you, or two ticked boxes, answers nothing.
- A reply works as it did. Only the login the comment mentions (`git.maintainer`, else the login `gh` acts as) answers, and it needs write access to the repo. Start the reply with the ruling's id, as `shep kelpie rule` does: `14 yes`, `14 no <note>`, `14 rework <note>` for a merge ruling. The number of the ruling's pull request or issue works in place of its id, such as `427 yes`. A bare `yes` answers nothing, so a conversational one never merges. A worker's question takes free text, with its id (not a pull request or issue number, which is part of the text) first only when several rulings wait on the thread.

Every answer, however given, is applied the same way, and the project manager reads that the ruling was answered in the board's events at its next wake.

A ruling that mentions no one, because `git.maintainer` is unset and `gh` cannot say who it acts as, is answered at the terminal only, and its comment says so. Anyone else's reply, reaction or tick, a bot's and a comment kelpie posted itself are left alone. Kelpie reacts with a thumbs up to the reply it took as the answer, and replies once to one it could not read, saying what to type. It reads the thread and each waiting ruling's comment about once a minute. The first answer wins, whether it comes from the thread, the terminal or ntfy, and the other is told the ruling is already answered. A worker's own `gh` may not comment: `gh pr comment`, `gh issue comment`, and any `gh pr` or `gh issue` verb but `create` and `edit` that carries a body or `--comment` (a `close` with a comment, a `review` with a body) are refused, as is any flag but `-R` before the verb and any `gh` word a shell expands (`$`, a backtick, `{`, `~`, a glob character) outside a `--body` or `--title`, since its `gh` is your login.

On an ntfy webhook, a ruling can also be answered in the topic (see [Answering in the topic](#answering-in-the-topic)).

With a project manager, it may answer a `stuck` ruling for you with a retry, or add its words to the ruling and post it again (see [The project manager](board.md#the-project-manager)).

## The webhook

A webhook posts each ruling where you will see it away from the terminal. Without one, a ruling shows only in the runner's log, `status` and `shep kelpie rule`, so you have to look.

It is the `[kelpie.webhook]` table of `dogs.toml` in the shepherd's home, for ntfy or Discord. Copy that table alone from [`kelpie-settings.example.toml`](https://github.com/shep-pm/shep-kelpie/blob/main/kelpie-settings.example.toml) and put your own URL in it: the file's `url` is a public ntfy.sh topic anyone can read. Its comments say what each key does, including a self-hosted ntfy's `ca_cert` and `token` and a `trusted` topic. `shep kelpie doctor` names the webhook rulings post to, and `shep kelpie doctor --test-alert` posts a test through the same settings.

### Answering in the topic

On an ntfy webhook, a ruling can be answered from the topic: run `shep kelpie totp` once and scan the QR code into an authenticator app, then reply with the line the alert ends on, such as `14 yes <code>`, with the app's code last. A reply takes the same answers as `shep kelpie rule`, and names the ruling the same ways: `427 yes <code>`, `#427 yes <code>` or `koji#427 yes <code>`. A reply by number written before the open ruling was raised is refused, saying the ruling changed and what it asks now, since a new ruling on the same pull request may ask something else. A reply by id, and an answer at the terminal, are taken as given. Anyone who can read the topic can read rulings, but only a reply with the code of the moment answers one, and each code answers once. A `trusted` topic is the exception: most replies there need no code (see [`kelpie-settings.example.toml`](https://github.com/shep-pm/shep-kelpie/blob/main/kelpie-settings.example.toml)). Five wrong codes turn answers off until `shep kelpie totp --unlock`, and `shep kelpie totp --rotate` replaces a secret that may have leaked.
