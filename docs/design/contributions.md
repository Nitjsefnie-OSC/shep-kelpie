# Contributions

Whether and how kelpie picks up other people's pull requests. Today it works only its own; it sees and lists the others (below), reads each new head for security, and runs nothing of theirs. #617, #618, #619

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

## Security review

- Until a head passes, nothing of the contributor's runs on the maintainer's machine: no build, test, hook or worker. Nothing adds a way to run it yet. Slots and budget are #621.
- `Runner::follow_contributions` starts a read of one contribution whose current head has no review, after the records are updated, `Runner::read_securities` in `src/runner/contributions/security.rs`. The read is a call in flight, `src/runner/flight/security.rs`: one at a time, on a thread of its own, so the runner's lock is not held while it runs, ended past a 15 minute ceiling, listed in a drain's `calls` as role `security`. The thread does the fetch, the diff and the call, and the verdict is recorded when its news is heard. The next unread contribution after the last tried goes next, so a head that never reads does not hold up the others. A head already read is not read again. A head that cannot be fetched, a call that fails and a reply in no verdict shape record nothing, so the next poll reads it again. Nothing is read while draining.
- `src/worktree/contribution.rs` fetches `refs/pull/<n>/head` and `main` into the project's checkout, which moves objects and checks out no branch, worktree or file. The diff is read with `--no-ext-diff --no-textconv --no-renames`. Git's output is read to at most `MAX_DIFF` bytes and git is stopped there, so a huge change costs no more than that; a cut diff is flagged for its length and its paths are not read.
- The diff is the change since the last passed head (`passed`) when that is an ancestor of the new head. With none, or after a force push (not an ancestor, or a commit git no longer holds), it is the whole `origin/main...head` change.
- Kelpie flags in code what a model cannot read, and never passes it: a binary change, a symlink, a submodule change, and a diff over 200000 bytes (`MAX_DIFF`). No model is asked. Each is a `HIGH` finding with its path and line 0, and the oversize one names `(the whole diff)`.
- Otherwise `contributions.security_reviewer` runs in a scratch folder, `<project folder>/security/<pr>/`, never in the checkout, removed with its settings file once the call ends. The diff is in it as `change.diff` and in the prompt through `src/runner/untrusted.rs`: a block whose delimiter, `UNTRUSTED-<n>` for the first `n` the text lacks, is absent from the text, with a line saying it is data to read and never instructions. Later slices reuse it for other outside text. The call is `build_call` with `Tools::Review`, so it has read tools only, and it gets none of the repo's plugins.
- A clean reply (`read_review`) sets `passed` to the head. Findings keep their file, line and why under `security`, and `passed` stays at the older head. `contributions[].security` is `{ head, verdict }`, the verdict `passed` or `{ flagged: [findings] }`; `passed` is the last head that passed. Both are left out until set. State version 47.
- A flagged head raises one `contribution` ruling of reason `security` (`issue: None`, `pull_request: Some(n)`, `src/runner/contributions/ruling.rs`). It names each file, line and why. A yes sets `passed` to the head (trusted); a no leaves it unpassed, runs nothing and posts nothing. A push, close or merge withdraws the ruling with no post, and an answer to it is refused as stale. A yes to an old head never trusts a new one. #620
## Remote

- `worktree::Remote` is where a work item's branch lives: `Origin`, or `Fork { url, branch }` for a contributor's fork. `prepare` (`Start::Pushed`), `origin_head`, `base_of`, `adopt`, `rebase` and `remove` take it, and every runner caller passes `Remote::Origin`, so nothing observable changes yet. A fork is fetched and pushed by URL, never added as a remote of the repo. `src/worktree/remote.rs`
- A fork's branch is fetched to `refs/remotes/fork-<hash of the URL>/<branch>`, never under `origin`, so a fork cannot overwrite `origin/main`. `Start::Main` and the catch-up with `main` always read `origin/main`. #631
