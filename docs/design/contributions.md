# Contributions

Whether and how kelpie picks up other people's pull requests. Today it works only its own; it sees and lists the others (below), reads each new head for security (`contribution-security.md`), hears what they say (`contribution-talk.md`), and runs nothing of theirs but the CI runs of a head that passed the security review (below). #617, #618, #619, #622, #625

## Settings

- `[app.dogs.kelpie.contributions]`, `#[serde(default)]`: an absent table is `pick = "off"`. `deny_unknown_fields`, so a mistyped key is refused naming it. `src/settings/contributions.rs`
- `pick`: `off` (default, kelpie's own pull requests only), `label` (those the maintainer labels for it) or `all`. Kebab-case, any other word refused.
- `reviewers`: ordered agent files whose role is `reviewer`, bots welcome, each once. Absent, it is the project's `agents.reviewers`, default included.
- `security_reviewer`: a reviewer agent file, `security-reviewer` when absent, shipped as `agents/security-reviewer.md`. It must run sessions: a command, endpoint or bot is refused.
- `quiet_days` (7) and `nudge_days` (3): whole days above 0. `nudge_days` must be below `quiet_days`: the nudge comes before a contribution counts as quiet. A zero is refused by the parser, the ordering by `Contributions::check`, called from `Settings::from_table`, so a bad pair is refused by key when the settings are read and a reread keeps the old settings.

## Checks at start

- `Settings::contribution_lineup` shares its body with `lineup` (`src/settings/reviewers.rs`), so the bot-order check applies. Its refusals name `contributions.reviewers` when that key is set, and `agents.reviewers` when it falls back.
- Agent files are checked only when `pick` is not `off`, so a project that never opts in needs none.
- `Runner::open` refuses a `pick` other than `off` when `ports.voice.covers(&remote)` is false: `SettingsError::Invalid` on `contributions.pick`. The representative would otherwise post as the maintainer, which no worker or reviewer may do. `pick = "off"` starts on any repo. `src/runner.rs`
- `Settings::security_reviewer` checks, whenever `pick` is not `off`, that `security_reviewer` has a file whose role is `reviewer` and that runs sessions. `Runner::open` calls it.

## Rereads

- `reread` keeps the running `contributions` and logs it as waiting for the next start, like `git.*`, whatever else changed: the checks above run only when the runner opens. `src/runner/reread.rs`

## Detection

- A contribution is an open pull request, not a draft, whose author is not the maintainer (`Runner::maintainer`), not the account kelpie acts as (`viewer`), and not a bot. Logins compare ignoring case. `Runner::is_contribution`, `src/runner/contributions.rs`
- Under `pick = "label"` it also carries `ready-for-agent`; under `"all"` it needs no label; under `"off"` nothing is one.
- The author decides, never the branch name or the fork flag: a fork can name its branch `kelpie/5`. `OpenPullRequest.author_is_bot` is the listing's `author { __typename }` being `Bot`. `src/adapters/gh/board.rs`
- `ProjectState.contributions` holds `Contribution { pull_request, author, head, seen }`, one per pull request, oldest first; `head` is the head commit, `seen` when that head was first seen. Absent in a file saved before version 40. `src/state/contributions.rs`
- `Runner::follow_contributions` runs once a `BOARD_POLL` before the rotation, so it runs with every slot held. It reads the pass's listing, which the board then shares. A new head updates the record; a pull request closed, merged, a draft again, or unlabelled under `label` drops it, with no post. A listing or login that cannot be read changes nothing.
- `adopt_waiting` leaves a labelled contribution alone when `pick` is not `off`. With `off` a labelled fork is refused as before. `rework_asked` still refuses a fork.
- `status` lists the records under `contributions`, left out when empty.

## Remote

- `worktree::Remote` is where a work item's branch lives: `Origin`, or `Fork { url, branch }` for a contributor's fork. `prepare` (`Start::Pushed`), `origin_head`, `base_of`, `adopt`, `rebase` and `remove` take it, and every runner caller passes `Remote::Origin`, so nothing observable changes yet. A fork is fetched and pushed by URL, never added as a remote of the repo. `src/worktree/remote.rs`
- A fork's branch is fetched to `refs/remotes/fork-<hash of the URL>/<branch>`, never under `origin`, so a fork cannot overwrite `origin/main`. `Start::Main` and the catch-up with `main` always read `origin/main`. #631

## CI on a trusted head

- GitHub holds a fork's first CI runs until a maintainer approves them, and approving runs the contributor's code. `Forge::approve_workflow_runs(repo, head)` lists the runs with `status=action_required` and that `head_sha`, drops any that is not both, and approves each. Kind `approve_workflow_runs`, REST. `src/adapters/gh/runs.rs`
- Kelpie approves as the maintainer's `gh` login, from the runner outside any sandbox, as it merges. If the App's token can approve (#636), moving to it is a follow-up.
- `follow_ci` (`src/runner/contributions/ci.rs`) runs at the end of `follow_contributions`, once a poll. A head is approved only when it equals `passed`: a flagged, unread or stale head never is, and a yes to a flagged head's ruling makes it `passed`, so the next poll approves it. A new push clears `ci`, so the new head waits for its own pass.
- `contributions[].ci` is `{ head, since, state }`, left out until the approval succeeds. `since` is the approval, and `state` is `waiting`, `green` or `{ red: [failed checks' names] }`.
- A rollup is trusted `CHECKS_SETTLE` (two minutes) after `since`, as at the gate, and only when `Forge::pull_request` reads the same head. No check or a pending one stays `waiting`. A skipped rollup is `green`: a contribution is never a draft. With `ci.block` off the state is `green` at the approval, as the gate passes a project without CI.
- A refused approval or an unreadable pull request logs and changes nothing: the next poll tries again, and no poll fails. A call that listed no held run is an approval, so a run that registers after it is not approved until a new head. State version 50.
- No reviewer reads a contribution before green, per ADR 0008. #623 enforces it.
