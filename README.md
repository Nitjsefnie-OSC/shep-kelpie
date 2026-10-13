# shep-kelpie

A [shep](https://github.com/shep-pm/shep) dog that runs coding agents (Claude Code, pi or Codex) on your projects, from a planned work item to a merged pull request. It holds the merge gate, the review budgets and the pacing in code, and calls a model only for the work and for judgement.

This is early. It changes without notice, and there is no release yet.

## How it works

```
 request ──> issue writer ──> issue: acceptance criteria, agent:<name>, ready-for-agent
                                 │
                                 v
 pick:   the project manager, or the board's rule (priority, then age)
                                 │
                                 v
 build:  one worker builds it inline and opens a draft pull request
                                 │
                                 v
 review: each listed reviewer reads it once, in order ──> one fix turn per reviewer
                                 │                          with any finding
                                 v
 CI:     red goes back to the worker (one rerun first if the log names no changed file);
         red again on the same head is a ruling
                                 │ green, on the latest main
                                 v
 merge:  a ruling under `ask`, kelpie's code under `auto`, as a merge commit
```

1. **Issue.** You and the issue writer turn a request into an issue with acceptance criteria, one pull request's worth, labelled with the agent that should build it (see [Writing issues](#writing-issues)).
2. **Pick.** A `ready-for-agent` issue joins the board. The project manager's agent picks the next one, or, without one, the board takes the first in its order, which starts as priority, then age, and opens a work item for it (see [The board](#the-board)).
3. **Build.** A worker, one of the project's implementers, builds it inline, opens a draft pull request and ends its turn (see [Building](#building)).
4. **Review.** Each reviewer in the project's list reads the pull request once, in order. One that finds anything, nits included, sends the worker all its findings for one fix turn, and the next reviewer reads the fix. No loop, no judge (see [The review](#the-review)).
5. **CI.** A red run goes back to the worker. A second red run on a head the worker left alone asks you.
6. **Merge.** Under `ask` you get a ruling; under `auto` kelpie merges once every gate passes. Either way it is a merge commit of the head the gates passed (see [CI and the merge](#ci-and-the-merge)).

Anything kelpie cannot settle itself is a [ruling](#rulings) for you. Each project runs as a sheep of your own shepherd, and every agent call it starts is one of that sheep's lambs (see [What shep does for it](#what-shep-does-for-it)).

## Getting started

From nothing to a worker on your repo. The examples use a project called `scratch`, on the repo `shep-pm/shep`.

### Before you start

You need these on your machine:

- macOS or Linux
- [shep](https://github.com/shep-pm/shep) 0.12, from 0.12.4, and Rust 1.88 or later, to build shep-kelpie
- Claude Code, signed in
- `node` and `npm`: `tools install` runs them, and so does every agent's sandbox
- on Linux, `bwrap` and `socat`, which the sandbox needs. There the sandbox fences a nested `.claude` or `.codex` folder only if it exists when a call starts (a new one is caught after the call), and a worker cannot reach the dog's lease door, so its `cargo-test` runs unleased. `shep kelpie doctor` says so
- `git`, and `gh` signed in to the account that opens the pull requests
- `curl` and `openssl`, which reach GitHub as kelpie's App and sign its tokens
- `pi` or `codex`, only to run an agent on that harness (see [Agents](#agents))
- a GitHub repo whose default branch is `main`, and a checkout of it

A runner is a sheep, so it gets the `PATH` your shepherd was started with, and it needs `claude`, `node`, `gh` and `git` on it. `shep kelpie doctor` runs in your shell, not the shepherd's, so it can pass while a runner fails to find one. Start the shepherd from a shell where `command -v claude node gh git` finds all four.

### 1. Install

There is no release binary yet. Build it from source:

```sh
cargo install --git https://github.com/shep-pm/shep-kelpie --locked
```

```
  Installing /path/to/bin/shep-kelpie
   Installed package `shep-kelpie v0.0.0 (https://github.com/shep-pm/shep-kelpie#61f0979f)` (executable `shep-kelpie`)
```

The binary is `shep-kelpie`, and `cargo install` puts it on your `PATH`.

### 2. Adopt it in your shepherd

```sh
shep adopt shep-kelpie --name kelpie
```

```
notice[dog_version]: kelpie reports version 0.0.0, shep protocol 11
notice[dog_channel]: kelpie asked for the shepherd channel, so it runs with channel and shutdown_with_message: `shep trigger kelpie <action>` reaches it, and shep stops it with a message rather than a signal
NAME    SOURCE   SHEPHERD  STATUS
kelpie  adopted  false     will start with the next shepherd
```

Keep the name `kelpie`: the `shep kelpie` commands reach the dog by it. If no shepherd is running, `shep muster` starts one, and shep-kelpie with it. Then `shep dogs` shows it:

```
ID  NAME    STATUS  PID    RESTARTS  EXIT  CPU  MEM   UPTIME  SOURCE
0   kelpie  online  23814  0         -     -    8.5M  4s      adopted
```

Adopt it once and leave it enabled. It is the dog that holds the leases every project's runner asks before a summon.

### 3. Install the tools

Every project needs them. Each agent runs inside the sandbox runtime they bring, and a runner won't start without it.

```sh
shep kelpie tools install
```

```
installed kelpie's tools in /path/to/.shep/kelpie/tools
```

It puts the sandbox runtime under shep-kelpie's home, `$SHEP_HOME/kelpie`, and the code graph server beside it (see [Code graph](#code-graph)), a release pinned by version and SHA-256 and fetched with `curl`. A machine the release has no build for gets no server, and the install says so. On macOS it also makes a copy of the runtime with the two keychain services (`com.apple.securityd.xpc` and `com.apple.SecurityServer`) cut from its profile, which the runtime has no setting for. A call on a repo kelpie's GitHub App covers runs under that copy, and refuses to start when the copy is missing, so nothing a worker runs can ask the keychain for a login the `security` command stored, without a prompt. A repo the App does not cover keeps the services: its worker pushes and runs `gh` with the login the keychain holds.

A covered call's `claude` cannot read its own login in the keychain, so it logs in with a token. Make it once, and keep it with `shep secret`:

```sh
claude setup-token
shep secret set CLAUDE_CODE_OAUTH_TOKEN
```

Then give the runner's sheep `CLAUDE_CODE_OAUTH_TOKEN = "{{secret:CLAUDE_CODE_OAUTH_TOKEN}}"` in its `env`. The runner hands the token to `claude` alone, in every role: the worker, the reviewers and the project manager. No other process it starts has the variable, and a Codex or pi call never gets it. It is one token the worker can read, where the keychain holds the full login. A covered call whose runner has no token does not start: it fails with the two commands, not with Claude Code's own "not logged in". `doctor` says MISSING for a covered project when the variable is set in neither the runner's `env` nor the shell you run it from. It reads from shep only which names in the runner's `env` are secret references, never the token, so a plain or empty value there does not count.

### 4. Add your project

In the checkout:

```sh
shep kelpie add
```

```
label `ready-for-agent`: already on shep-pm/shep
label `ready-for-human`: already on shep-pm/shep
label `in-progress`: already on shep-pm/shep
label `review please`: already on shep-pm/shep
agent files: wrote sonnet-high, opus-high, defect-hunter, standards, spec, coderabbit, cubic, codex, issue-writer, pm, advisor, researcher in /path/to/.shep/kelpie/agents
runner `scratch`: added with its settings, stopped
`shep kelpie start scratch` runs it
```

On a repo without those four labels, `add` makes them. The runner puts `in-progress` on an issue while a work item has it. It writes kelpie's own [agent files](#agents) where they are missing, and never over one you edited. The project is named after the repo, or `shep kelpie add <name>`, as `scratch` was here.

`add` writes the project's settings with these defaults:

- `git.checkout` and `git.remote`: the checkout and the GitHub repo its `origin` names
- `git.merging = "ask"`: you rule on every merge
- `git.issues = "ask"`: you rule before deferred findings are filed as issues
- `ci.block` is on when the checkout has `.github/workflows`, and `ci.fix_attempts = -1` puts no cap on the worker's fix turns for red runs
- `concurrency.active_items = 1`: one work item working at a time. With more, their turns and reviews run at the same time, one call at a time per work item. A work item parked on a ruling gives its slot up
- `concurrency.pending_rulings = 2`: once two work items wait on your rulings, the board opens nothing new. It does not cap rulings
- `budget.per_item_usd = 10`: what one work item may spend, in dollars, on its worker's and reviewers' priced calls together. A call whose harness reports no dollars counts nothing. Past it, the work item raises a `stuck` ruling for `over-budget` before its worker's next turn starts, and a turn already running is never cut off. A yes doubles the work item's cap, and again while it is below the spend (one long turn can cost more than the doubled cap), and the worker carries on. A `no <note>` sends the worker the note ahead of the turn that was due, and raises the cap the same. A change to the setting takes effect at once, and `status` and the board show each open work item's spend against its cap. The review rounds are not held back by it. A cap below one cent is refused
- `agents.implementers = ["sonnet-high"]`
- `agents.reviewers = ["defect-hunter"]`, with `qwen` first when `~/.claude/scripts/qwen-review.sh` exists, and no review bot
- `pacing.enabled = true`
- `worker.allowed_domains = []`
- `worker.turn_timeout = 60`, in minutes
- `worker.compact_window = 200000`, in tokens

The checkout is the project's repo, and shep-kelpie runs `git fetch`, `git worktree` and `git branch` against its `.git`. Its worktrees, build folders and state go under `$SHEP_HOME/kelpie/<project>`, never inside it. Add from a clone you don't work in if you'd rather keep your own checkout out of it.

### 5. Register kelpie's GitHub App

shep-kelpie posts its rulings, notices and review rounds on GitHub as an App of its own that you register for yourself, so they reach you through GitHub's notifications: GitHub never notifies you of what your own login posts. Each ruling is a comment on the work item's pull request, or on its issue while it has none, that mentions you and says how to answer. A notice that a pull request merged is a comment on it, and each round of a reviewer of kelpie's own is posted on the pull request as it ends, with its findings or that it found none. The issues and labels shep-kelpie makes come from the App too. Reply to the comment to answer it (see [Rulings](#rulings)), or use `shep kelpie rule`. Without an App, shep-kelpie posts as it always did, from your own login. For the account that owns the repo:

```sh
shep kelpie github setup
```

For a repo an organization owns, `shep kelpie github setup --org <org>`. It opens GitHub's page for a new App, filled in: the name `kelpie-<owner>`, no webhook, and only these permissions: contents, workflows, issues and pull requests to write, metadata to read. Click to create it, and GitHub sends you back to shep-kelpie, which keeps the App and opens the page that installs it. Install it on the repos shep-kelpie works. If the browser does not open, the command prints the address to open by hand.

App names are shared by all of GitHub, so if yours is taken, stop with Ctrl-C and run again with `--name <another name>`. Run it once for each owner: an App you register yourself installs only on the account that owns it. Running it again for an owner that has one says so and changes nothing, unless you give `--replace`. Before replacing, delete the old App on GitHub (its owner's settings, Developer settings, GitHub Apps), since its name is still taken while it exists, or give the new one `--name`. `shep kelpie github setup --help` lists the flags.

shep-kelpie keeps the App's private key in its home, `$SHEP_HOME/kelpie` or the folder `KELPIE_HOME` names, under `github/<owner>`, readable by you alone, and no worker can read it. Kelpie's own token holds issues and pull requests alone, so it cannot push, change code or merge: merging stays on your own `gh` login, and so do a review bot's summons and the labels on a pull request, since a bot answers a person.

On a repo the App covers, a worker never holds your `gh` login. Each of its calls gets a token minted for that repo alone with the permission to push, contents, and where the App holds it the permission to change workflow files, and nothing that can comment, review, approve or merge. Its `gh` and git use that token alone, never your `~/.config/gh` or the login `gh` keeps in the macOS keychain, and it may not read those or run `security`. Kelpie opens and edits the worker's pull request for it, from a file the worker writes. The token can push any branch of the repo, so protect `main` on GitHub with a branch protection rule or a ruleset: that is what stops a push to it. An App you registered before this needs the permission added: in its settings on GitHub, set Repository permissions, Contents, to Read and write, then accept the change on the installation; `doctor` names it as missing until you do. The `workflows` permission also lets a worker push a change to `.github/workflows`, so it can change CI, which runs with the repo's secrets: review a workflow change as you would any change to what runs with your secrets. An App without `workflows` gives workers a token that cannot push a workflow file, and `doctor` says which your workers have; to add it to an App you registered earlier, set Repository permissions, Workflows, to Read and write, then accept the change on the installation. On a repo no App covers, workers hold your `gh` login as before, and `doctor` says so.

On a repo the App covers, kelpie's reads of the board, the pull request, its review and its checks go through the App too, with a rate budget of its own, so they leave your login's. The App needs the read permissions Issues, Pull requests, Checks and Commit statuses; a new App is registered with them, and an App registered before this lacks Checks and Commit statuses, which `doctor` names. Until you add them (Repository permissions, set to Read-only, then accept the change on the installation), the reads that need them, a pull request's checks and a review bot's statuses among them, go through your `gh` login, and the runner says which permission was missing, once. A read the App is refused, or that finds the App's rate limit used up, goes through your login too, and the runner says so once. Everything else, writes included, stays as it was: the viewer, access checks, comments read since a time, failed run logs, and the merge. A repo no App covers is read by your login as before.

A ruling mentions the login in `git.maintainer`. Left out, it mentions the login `gh` acts as (`gh api user`), asked once a run, and `doctor` shows it as `(from gh)`. Set `git.maintainer` when `gh` acts as an account that is not you, such as a bot. If `gh` cannot say who it acts as, a ruling is posted without a mention, which notifies no one: `doctor` marks that `MISSING`, and the runner says so once a run in its log. An App newly installed on a repo is noticed within ten minutes.

### 6. Check the machine

```sh
shep kelpie doctor
```

```
ok       claude: installed and logged in
ok       gh: logged in as <you>
ok       sandbox: the sandbox runtime can run
ok       shepherd: shep 0.12.4 at /path/to/.shep
ok       dog: kelpie's dog is running and has named itself
ok       scratch: checkout: /path/to/checkout is a git checkout with an origin
ok       scratch: implementers: an issue with no `agent:` label runs on sonnet-high
ok       scratch: push access: may push to shep-pm/shep
ok       scratch: labels: shep-pm/shep has `ready-for-agent`, `ready-for-human`, `in-progress`
ok       scratch: github app: kelpie's App is installed on shep-pm/shep and mints a token
ok       scratch: workers: a worker pushes with a token for shep-pm/shep alone, which cannot comment, review, approve or merge; it can push any branch, so protect `main` on GitHub
ok       scratch: reviewers: each pull request is read by defect-hunter
ok       scratch: rulings: no webhook, so rulings reach you only in the log, `status` and `shep kelpie rule`
nothing a project needs is missing
```

Each `MISSING` line names its fix, and `doctor` exits non-zero until they are done. A ruling is a question shep-kelpie cannot settle itself, such as a merge, and it must reach you. With no webhook it shows only in the runner's log, `shep kelpie status` and `shep kelpie rule`, so you have to look. To be told, add a `[kelpie.webhook]` table for ntfy or Discord to `dogs.toml` in the shepherd's home. Copy that table alone from `kelpie-settings.example.toml`, and put your own URL in it: the file's `url` is a public ntfy.sh topic anyone can read.

Run `shep kelpie doctor` again. With a webhook set it ends:

```
ok       scratch: rulings: rulings post to the ntfy webhook
nothing a project needs is missing
```

### 7. Open the sandbox to your registries

A worker's sandbox reaches `github.com`, `api.github.com` and the model's API, and nothing else. `add` leaves `allowed_domains` empty, so a first issue on a repo with a cold cache can't fetch crates or npm packages. Set it with `set`, which writes the project's table on its runner sheep. A list is a TOML value, so quote it for the shell:

```sh
shep kelpie set worker.allowed_domains '["crates.io", "index.crates.io", "static.crates.io"]'
```

A value that does not fit the settings is refused with the reason, and nothing is written. `shep kelpie get worker.allowed_domains` reads it back.

It ends up holding:

```toml
[app.dogs.kelpie.worker]
allowed_domains = ["crates.io", "index.crates.io", "static.crates.io"]
```

For npm, that is `registry.npmjs.org`. An entry is a host, with one optional leading `*.` label to take its subdomains (`*.crates.io`) and an optional `:port`. A `*` anywhere else, such as `productionresultssa*.blob.core.windows.net`, a scheme and a path are refused when the settings load, since the sandbox would refuse to start on them: a live reread keeps the running settings and names the entry. `researcher.allowed_domains` takes the same entries. A change reaches a running runner at its next wake.

### 8. Start it, and label a first issue

```sh
shep kelpie start
```

It starts the project's runner, and the project runs while it does: `shep start scratch` and lookout's start do the same, and `shep ls` shows it `online`. The runner reads the board from its first pass.

Put `ready-for-agent` on an issue that says what done looks like, with acceptance criteria, or have the issue writer write it (see [Writing issues](#writing-issues)). The runner gives it to a worker, which opens a draft pull request. Then the review runs, each listed reviewer once, then CI. Last, you get a ruling before the merge. `shep kelpie status` shows every project. [The board](#the-board) says what decides whether, and when, an issue starts.

### 9. Answer a ruling

A ruling reaches the webhook you set in step 6, or waits in `status` and `shep kelpie rule`. Answer it from the terminal:

```sh
shep kelpie rule          # lists the rulings waiting
shep kelpie rule 14 yes
shep kelpie rule 14 no rename the flag
shep kelpie rule 427 yes  # the number of the ruling's pull request or issue
```

On ntfy you can also reply in the topic, after a one-time `shep kelpie totp`. [Rulings](#rulings) has the six kinds and how to answer each.

A self-hosted ntfy takes three more keys in `[kelpie.webhook]`, each optional:

```toml
[kelpie.webhook]
kind = "ntfy"
url = "http://ntfy.your-tailnet.ts.net/your-private-topic"
ca_cert = "/etc/kelpie/ca.pem"
token = "tk_..."
```

- `url` may be `http://` when its host is a Tailscale address (100.64.0.0/10) or a `.ts.net` name, since the tailnet encrypts it. Any other `http://` host is refused, except this machine, and so is a URL with a user name or password in it.
- `ca_cert` is the absolute path of your private CA's certificate file, which curl trusts for the post and for reading replies. The dog does not start when the file is missing or unreadable, and says `webhook.ca_cert` is the key.
- `token` is an ntfy access token, sent as `Authorization: Bearer <token>` on the post and on reading replies. Like the URL, it is a credential and no error, log line or status shows it.

`shep kelpie doctor --test-alert` posts through the same settings. Replies still need the authenticator code, unless the topic is `trusted`:

```toml
[kelpie.webhook]
token = "tk_..."
trusted = true
```

- `trusted = true` is your word that the server lets only that `token` post to the topic, such as an `ntfy access` rule denying everyone else. Kelpie cannot see the server's rules. The dog does not start with `trusted` and no `token`, and says `webhook.trusted` is the key.
- On a trusted topic `14 no <note>`, `14 rework <note>`, a yes on any ruling but a merge and a worker's answer need no code. A yes on a merge ruling still takes the code, since the topic alone never merges.
- Five wrong codes still turn coded answers off until `shep kelpie totp --unlock`, and replies that need no code go on working.
- The alert's last line shows the reply each ruling takes, and `shep kelpie doctor` prints a `replies` line while the topic is trusted.

### 10. Pause, and find the log

```sh
shep kelpie pause
```

The runner starts no new call, the calls already running finish and a merge found in flight lands, then shep stops it, so `shep ls` shows it `stopped`. It says what it waits on meanwhile. A session you attached keeps running in your terminal. A ruling still waiting can be answered with `shep kelpie rule` while it is stopped, and the runner acts on the answer when it starts. `shep kelpie start` runs it again. A merge a ruling starts in the moment between that check and the stop can still be cut short, and its work item resumes when the runner starts. If a wait runs out, you interrupt it or shep refuses the stop, the runner keeps running. A runner's log is `shep bleats <project>`, and the dog's is `shep bleats kelpie`.

To wind a project down without watching it, finish it instead:

```sh
shep kelpie finish
```

`pause` stops after the calls running now, mid-item, and `drain` only holds back new calls and never stops anything. `finish` lets every open work item run on to its end, through review, CI, rulings and the merge, picks nothing new from the board, then stops the runner the way `pause` does once the last item has merged, closed or been dropped and any merge notice has been posted. An item parked on a ruling is still open, so finishing waits until its ruling is resolved. With nothing open it stops at once. Meanwhile `status` shows `run: finishing` with the items still open, `shep kelpie status` marks the project `(finishing)`, the board briefing says so, and `add <issue>`, `adopt <pr>` and `rework <pr>` are refused. Pull requests already adopted and waiting for a slot stay waiting until the next start, and `finish` names them. A runner restarted while finishing comes back finishing, and `shep kelpie start` cancels it, so the board picks again, except while the runner is already stopping itself: start it once it has stopped. A `pause` during finishing is the stronger stop: it ends finishing too, so the runner comes back picking when it starts.

## The flow

### Writing issues

The issue writer turns a request into issues an agent can build from. It reads the repo, scopes the request to one pull request's worth, or splits it into the smallest pieces that each work, test and ship on its own, writes acceptance criteria into every issue, and labels each `agent:<name>` with one of the project's implementers.

```sh
shep kelpie issue "let a project be paused from lookout"
shep kelpie issue --interactive "let a project be paused from lookout"
```

On its own, it runs one fresh session in a detached checkout of `main`, files what it writes as `ready-for-human`, and ends with the list of what it filed. shep-kelpie reads each issue back and prints it, with its `agent:` label and the issue it is a sub-issue of. Read them, then label them `ready-for-agent`. An issue without acceptance criteria, its status label or exactly one `agent:` label naming a listed implementer is named with what it lacks, and the command exits non-zero. A session that fails, or ends without the list, keeps its checkout and prints the `claude --resume` line that picks it up.

On its own there is no one to grill, so a decision the repo's docs and code do not settle is left open, not guessed. The issue it files ends with an `## Open questions` section: each decision as a numbered `❓ **Q1** - **<title>**: ...` question in the format of the `grilling` skill, with a `➡️` recommended answer, so you can answer on the issue before you label it `ready-for-agent`. An issue with nothing left open has no such section.

With `--interactive` it runs `claude` in your terminal, with the issue writer's prompt appended, and the first message `/mattpocock:grill-with-docs <request>`. The `grill-with-docs` skill (`grilling` with `domain-modeling`) interviews you on the request in rounds, each question with a recommended answer, until nothing is left silently assumed. It files nothing until you confirm you share an understanding, and then files what you agree as `ready-for-agent`, onto the board. It is your session, not a runner's: shep-kelpie starts it, and Claude Code asks you before each command it runs and each file it writes.

The session runs in a checkout of a docs branch, `kelpie/docs-<id>`, cut from a fresh `origin/main`, not in your own checkout. As decisions settle, `domain-modeling` writes terms to `GLOSSARY.md` and ADRs to `docs/adr/` there, and its file tools refuse any other file. When the session ends, shep-kelpie commits what it wrote, pushes the branch, opens a draft pull request beside the issues and comments on each issue the session filed with a link to it. A session that wrote no docs leaves no branch. One that somehow changed another file commits nothing and keeps its checkout, which the command names.

In both modes the facts of a request come from the repo, never from you: where a skill would send a sub-agent to look something up, the writer, which has none, reads the repo with its own Read, Grep and Glob. It asks you only for decisions.

Either way the session reads the checkout with Read, Grep and Glob, and nothing outside it: not gh's config, Claude Code's own files or your shell history. On its own it edits no file, and interactively it edits only those two docs paths. Its Bash runs only what `kelpie guard` lists for it: `gh issue create`, `gh issue view <n>` and `gh issue list` on this repo, and, for the issues it filed in this session, `gh issue edit` on labels and `gh api` on their ids and sub-issue and blocked-by links, each as plain words, with a body given as a heredoc behind a quoted delimiter (`--body-file - <<'EOF'`). It runs no git. A hook after each command records the issues it filed and the ids it read, and the guard refuses an edit or a link on any other. The guard also refuses an issue without acceptance criteria, without the status label, or without exactly one listed `agent:` label, and one whose title or body names a path on this machine. On its own the session also runs in the sandbox, with nothing to write but its own scratch folder and only GitHub to reach. Each implementer's `agent:` label is made on the repo the first time the issue writer needs it.

The issue writer is the agent file `agents.issue_writer` names, `issue-writer` when it names none: edit its body to change its prompt, or its `model` and `effort`, or name a file of your own whose role is `issue-writer`.

The runner asks it too. When the board would open a `ready-for-agent` issue with no `agent:` label and the project lists more than one implementer, one issue writer call reads the issue and the repo, with read tools only, and names the implementer that should build it. shep-kelpie puts that `agent:` label on the issue, and the board opens it on its next poll. The call only picks: it files and labels nothing itself, whatever the agent file says of filing. Its reply's last line of JSON names the agent. It waits on Claude's window like any new work item, even when every role the project lists is local, and `add` refuses the issue while the call runs. The call's line goes in the usage ledger with the issue's number. A call stopped with the runner raises nothing, and the next run asks again. A call that fails, or names no listed implementer, raises a `stuck` ruling on the issue instead of guessing: the board passes the issue over, listed under `skipped` as `unlabelled`, until you answer it. Label the issue, then answer `yes`; an issue still unlabelled goes to the issue writer again. A `no` is refused, since there is no worker to take its note. A label you put on while the call runs stands, and the issue writer's pick goes unused. One labelled while the ruling waits opens anyway, and that clears the ruling. With one implementer listed there is nothing to pick, and an unlabelled issue runs on it.

### The board

Issues labelled `ready-for-agent` are the board. What decides whether, and when, an issue starts:

- The board skips an issue that is assigned to anyone or already has an open pull request, and one blocked by an issue that is still open. Don't assign it to yourself
- The board also skips a ready issue labelled `in-progress` that this runner holds no work item for, since an earlier worker holds it, and `status` lists it under `skipped` as `in-progress`. Take the label off to release it. A restart still resumes the runner's own items
- An issue with sub-issues is never worked itself: its sub-issues are, and shep-kelpie closes it once every sub-issue is closed
- The board keeps the rest in an order, and takes the first it can start. A newly ready issue joins after the last issue of its priority, `priority: P0` to `P4` then none, and the oldest goes first when several join at once. An issue that closes or loses `ready-for-agent` leaves the order. With no label changes, that is the rule of priority then age, and `status` lists the first five under `order`. Changing the priority label of an issue already in the order moves it to just after the last issue of its new priority. An `agent:<name>` label, such as `agent:opus-high`, picks the agent that works it from those the project lists in `agents.implementers` (see [Agents](#agents)), and a label naming one it does not list keeps the issue off the board. With a `!` at its end, as `agent:opus-high!`, it also pins the work item to that agent, which never falls back. An issue with no `agent:` label goes to the issue writer first when the project lists more than one implementer (see [Writing issues](#writing-issues)). An old `worker:` label is no longer read: that issue counts as unlabelled, and the runner's log says so as its work item opens
- You order that list. `shep kelpie board` prints it numbered, each issue with its title, its priority, and `blocked` or `held` when the board cannot take it yet. `board move <issue> <position>` puts an issue at that place, counting from 1, and `board top <issue>` puts it first; the issues between shift by one, and the runner takes the new order from its next pick, restarts included. Only a ready issue is in the list. A move of any other, or to a position past the last or before the first, is refused and changes nothing. `board` lists what the runner last read, so it is empty until the first read of the board. With `--json` each prints the runner's object, the list under `board`
- With a project manager set up (see [The project manager](#the-project-manager)), it picks among two or more issues the board could start, and may hold some back. Without one, or while it is down, the order above picks
- A slot under `concurrency.active_items` bounds model calls. A work item takes one when it opens and keeps it through CI and the merge, but one parked on a ruling gives it up, so the board opens the next issue while it waits. Once `concurrency.pending_rulings` work items are parked, the board opens nothing new, and the alert for the ruling that filled it says so. It is the point where the board stops, not a hard limit: items already working can still park past it, and `add` ignores it. `concurrency.pending_rulings = 0` stops the board while any ruling waits, and with none waiting the board opens work as ever. A merged pull request parked on its follow-up ruling counts toward neither cap. An issue whose body names a file the branch of a parked item not yet merged changes waits for that item, and `status` lists it under `skipped` as `overlap`. One whose paths the board has not read yet, or every one while such a branch's files are not known, waits a pass and is listed as `paths-unread`
- Once you answer its ruling, a work item goes on without a slot through anything that calls no model: CI, a merge, or its end. When it needs a worker's turn or a review again, such as for your `no <note>` or a red CI run, it waits for the next slot to free, ahead of any new issue. `status` lists the open items' issues under `working`, `waiting_for_slot` and `parked`
- No turn starts while Claude's 5-hour window is at 50% or more, and no new work item starts once today's share of the week is spent. `shep kelpie status` says why under `pacer`, and `enabled = false` in the project's `[app.dogs.kelpie.pacing]` turns both off

`shep kelpie add <issue>` opens a work item for an issue at once, without the label, ahead of the board's order. It queues nothing: while every slot under `concurrency.active_items` is held or waited for, `add <issue>` is refused.

On a pull request kelpie opened, `ready-for-agent` or a review requesting changes starts a rework of it, the same as `shep kelpie rework <pr>`. On any other open pull request of kelpie's account, `ready-for-agent` adopts it, the same as `shep kelpie adopt <pr>`. Adoptions and reworks go before any issue. Kelpie puts `ready-for-human` on each pull request it hands back.

The runner reads the board from GitHub at most once a minute, however often something wakes it, and at once after something that changes what the board would see, such as a work item ending, a ruling answered, an `add` or an `adopt`. `status` and `drain` answer without making it read anything, so `upgrade` and `pause` can ask them twice a second while they wait. A work item's step or a board read that fails is tried again after 15 seconds, then twice as long each time up to 10 minutes. When GitHub says the account's rate limit is used up, the runner makes no GitHub call until the limit resets, says so once in its log, and `status` shows when under `forge_held_until`.

### Building

A worker builds its work item inline, in a worktree of its own cut from `origin/main`, on the agent its issue's `agent:` label names or the project's default implementer. It commits, pushes, opens a draft pull request that ends `Resolves #<issue>`, and ends its turn. When the repo has a pull request template, or its owner's public `.github` repository has one for repos that have none, it fills that. A project's `worker.instructions_file` adds rules the repo's own docs don't carry.

Every turn after the first that changes the pull request's diff ends its prompt by asking the worker to update the pull request's body to match the head it pushes: what changed, the files touched, the test evidence and the decisions. That covers a review fix, a CI fix, a merge queue removal, a no or a rework from you, and a rework or adoption of a pull request. A worker on your login runs `gh pr edit`, and a worker whose credential cannot edit the pull request rewrites the file kelpie sets the body from. A turn that only merges `main` in does not ask, since it changes no part of the pull request's own change.

A worker whose first turn ends with no pull request and no question is sent back once, and asks you for a ruling if it stops short again; once you answer a question it asks, the next turn that stops short is sent back once again. A worker that needs a decision only you can make ends its turn on a question, which reaches you as a ruling, and your answer is its next turn.

A work item with no pull request ends when its issue is closed and it holds no work: no commit on its branch or in its worktree that `main` lacks, and no file left uncommitted. The worker found nothing to change, or you closed the issue. It ends as soon as the worker's turn does, with no ruling. Parked on a ruling, its issue is read at most once every five minutes, and it ends once a read finds the issue closed, its ruling withdrawn. The log, the webhook (when one is set), `status`, the board and `usage` say the issue was closed with no change, and its history entry has `closed` set, `merged` not. shep-kelpie cannot tell who closed the issue, so a worker that closes it on its own ends its work item the same way. Work on the branch or in the worktree, or an open pull request that closes the issue, leaves the work item to go on as before. A turn still running at `worker.turn_timeout` minutes is ended and asks you whether to go on.

Each worker runs inside the sandbox with its fence: it writes only its worktree and build folder, reaches only GitHub (including the hosts its log and artifact downloads redirect to), the model's API and the project's `allowed_domains`, and never merges, marks a pull request ready or summons a review bot. `kelpie confine` checks every file write and `kelpie guard` every command. Workers run their test suites under the `cargo-test` lease (see [Leases](#leases)).

Kelpie keeps what is private off the forge. It refuses, and never sends, any commit message, issue, comment, reply or pull request text that names:

- a path that names you: your home folder, kelpie's home and the project's checkout, `/home/<name>`, `C:\Users\<name>`, and the scratch folders `/private/tmp` and `/var/folders`
- an address on a local network, such as `192.168.1.20` or `host.local`
- in a post kelpie makes to a public repo, a project of your flock whose repo GitHub marks private, as `owner/name` or by its bare name. The `gh` calls and commits of a worker are not held to this rule

A `~/` path goes through, since it is the same folder on every machine that runs shep. A post to a repo GitHub marks private may name another private project. A refusal says which rule fired and never repeats the text. The flock's repos are read when the runner starts, so a project added later is covered after the runner restarts.

#### Skills

Every step shep-kelpie drives an agent through runs a skill, by default from [mattpocock/skills](https://github.com/mattpocock/skills) (MIT). shep-kelpie vendors the ones it uses in `skills/`, pinned to one upstream commit with its licence, and writes them out as a Claude Code plugin when a runner starts. A project installs nothing.

| step | default skill | where it runs |
|---|---|---|
| `triage` | `triage` | not driven yet |
| `spec` | `to-spec` | not driven yet |
| `implement` | `implement` | the worker's first turn on an issue |
| `tests` | `tdd` | named in the worker's instructions |
| `ci_fix` | `diagnosing-bugs` | the worker's turn on a red CI run |
| `pr` | `pr` | named in the worker's instructions, unless the repo or its owner's `.github` repository has a pull request template |
| `reset` | `handoff` | not driven yet |
| `retro` | off, `retro` with `{ kind = "kelpie" }` | the project manager's batch retro, one call over the work items finished since the last (see Retros) |
| none, not a `[skills]` step | `code-review` | the `standards` and `spec` reviewers, when a project lists them (see [Agents](#agents)) |

`shep kelpie issue --interactive` starts with `grill-with-docs` (see [Writing issues](#writing-issues)). It is no `[skills]` step: it always runs kelpie's vendored copy.

Some vendored skills call others, and those are vendored too: `codebase-design` (from `tdd`), `grilling` and `domain-modeling` (from `triage` and `grill-with-docs`) and `writing-for-agents` (from `retro`). No step names them, and a test fails when a vendored skill or its docs call one that is not vendored, except `setup-matt-pocock-skills`, which a project runs once to set itself up and a worker never runs.

A step that runs a skill starts its prompt with the skill's slash command, such as `/mattpocock:implement`, and shep-kelpie's own prompt follows as its arguments. To override one, set it in the project's `[app.dogs.kelpie.skills]` table:

- `{ kind = "path", path = "..." }`: a skill folder with a `SKILL.md`, copied into a plugin of its own, `kelpie-<step>`
- `{ kind = "plugin", plugin = "...", skill = "..." }`: a skill in a Claude Code plugin's folder
- `{ kind = "none" }`: shep-kelpie's own prompt, no skill

A skill that can't load runs shep-kelpie's own prompt instead. The runner logs why, and `status` shows it under `skills`.

#### Repo plugins

A plugin the repo turns on in its `.claude/settings.json` reaches every agent shep-kelpie runs there, which a headless Claude Code run does not do by itself: it loads only installed plugins, and workers run with `--setting-sources project`. So `rust-house-style:rust-house-style`, named in a repo's docs, is a skill its workers can call.

- As a runner starts, when it rereads its settings, and when the default branch it last fetched has moved, it reads `.claude/settings.json` on the checkout's default branch, never a worker's branch, so a pull request cannot turn a plugin on for its own review.
- Each `enabledPlugins` entry set to `true` is looked up in `extraKnownMarketplaces`. A `github` or `git` source is cloned, or fetched if already there, with the runner's own `git` into `<kelpie home>/marketplaces/<name>`. A `directory` source is a relative path to a folder inside the checkout, read as the default branch holds it (the checkout itself is never moved) and copied to `<kelpie home>/marketplaces/<name>`: an absolute path and a `..` are refused, and a link in the folder is left out.
- The plugin's folder is found in the marketplace's `.claude-plugin/marketplace.json`. Its `skills/` and a trimmed manifest are copied into a plugin of the same name under `<project folder>/skills/repo/<name>`, rewritten at every start, so its skills keep their names.
- Every call gets it as one more `--plugin-dir`: the worker's turns, the reviewers, the issue writer, the project manager, the advisor and the researcher. `[app.dogs.kelpie.repo_setup]` narrows that. `shep kelpie issue` is not the runner, so its headless issue writer loads the plugins itself, into a folder of its own that it removes when the session ends, and `shep kelpie issue --interactive` is your own `claude`, which loads none of them.
- A plugin's `skills/`, `agents/`, `commands/` and `hooks/` are copied as files and folders, and `repo_setup.parts` narrows which: a link inside a plugin is left out. Hooks are read from `hooks/hooks.json`, so a script a hook runs must sit in `hooks/` too, and a hook command reaches it through `${CLAUDE_PLUGIN_ROOT}`. A plugin's hooks run beside the hooks in the call's settings file, inside the call's fence, and cannot replace or turn off kelpie's own. A plugin's MCP servers load too, as [MCP servers](#mcp-servers) says. A second plugin of a name an earlier one supplies, in the order the repo lists them, is skipped. A change to a plugin's files alone is copied over too. A `github` or `git` marketplace is fetched again only when one of those happens, not on a timer. A plugin with none of the parts `parts` loads is skipped. A plugin whose marketplace entry points outside the marketplace is skipped too.

A plugin that cannot load is skipped, the runner logs why and the run goes on: no `extraKnownMarketplaces` entry, a failed fetch with no copy yet, no such plugin in the marketplace, none of the parts that load, or a name shep-kelpie keeps for itself (`mattpocock`, `kelpie-<step>`). A failed fetch keeps the last good copy and `status` marks the plugin stale. `status` lists each plugin under `plugins` with its skills or why it was skipped, and `doctor` reports the same under `repo plugins` from the marketplaces already fetched, without fetching any.

##### `repo_setup`

A project narrows what reaches each agent, and adds a plugin the repo does not name, in its `[app.dogs.kelpie.repo_setup]` table. Every key is optional, an unknown key stops the runner naming it, and a change is read as the runner rereads its settings.

- `plugins`: `true` by default. `false` loads none of the plugins the repo turns on, and `extra` still loads.
- `own`: `true` by default. The repo's own `.claude/skills/`, `.claude/agents/`, `.claude/commands/`, the `hooks` of `.claude/settings.json` and the servers of `.mcp.json`, read from the default branch and written as one more plugin, `kelpie-repo`. `false` leaves them out and keeps the plugins, `extra` ones included. See [The repo's own files](#the-repos-own-files).
- `roles`: the calls that get plugins and the repo's own files, from `worker`, `reviewer`, `issue-writer`, `pm`, `advisor` and `researcher`. All of them by default. `roles = ["worker"]` keeps plugins and the repo's own files from the reviewers, the issue writer, the project manager, the advisor and the researcher. It narrows `extra` as well.
- `parts`: which parts of a plugin, and of the repo's own files, load, from `skills`, `agents`, `commands`, `hooks` and `mcp`. All of them by default. `parts = ["skills"]` copies only a plugin's skills, and leaves its agents, commands and hooks out, and the same for the repo's own files. `mcp` picks the MCP servers of a plugin and of the repo's `.mcp.json`, see [MCP servers](#mcp-servers). `parts` without `mcp` leaves both out.
- `skip`: plugins, as `name@marketplace`, left out though the repo turns them on. A skipped plugin is not listed in `status`.
- `extra`: plugins the repo does not turn on, each as `name@marketplace` with its marketplace's source as one string: `github:owner/name`, `git:<url>` or `directory:<folder inside the checkout>`, and `#<branch or tag>` after a `github` or `git` address. The kinds are those of the `source` in an `extraKnownMarketplaces` entry. A marketplace given here takes the place of the repo's of the same name, and two plugins of one marketplace must give the same source. An extra plugin that cannot load is skipped with its reason, as the repo's are.

```toml
[app.dogs.kelpie.repo_setup]
roles = ["worker", "reviewer"]
parts = ["skills", "agents"]
skip = ["noisy@house"]
extra = { "style@lab" = "github:acme/lab#v2" }
```

##### The repo's own files

A repo sets Claude Code up for everyone who works in it without a plugin too: skills in `.claude/skills/`, sub-agents in `.claude/agents/`, commands in `.claude/commands/` and hooks in `.claude/settings.json`. A worker loads them because its turn runs in the worktree, but the project manager, the advisor, the researcher and the issue writer run in folders of their own, so shep-kelpie hands them over.

- It reads the four from the checkout's default branch, never a worker's branch, and writes them as one more plugin, `kelpie-repo`, under `<project folder>/skills/repo/kelpie-repo`, rewritten at every start and whenever the default branch moves. A plugin's name carries a namespace, so its skills are invoked as `kelpie-repo:<skill>`. A repo's plugin cannot take that name.
- Every call gets it as one more `--plugin-dir`, as it gets a plugin, for the roles `repo_setup.roles` names. A Claude Code call that runs in a worktree whose agent files are exactly the default branch's already loads them there, and does not get the plugin as well: a worker's turns and the reviewers get none, so no skill or hook shows up twice. The issue writer, which runs in a view of the default branch, is such a call too.
- The `hooks` of `.claude/settings.json` go to the plugin's `hooks/hooks.json`. They run beside the hooks in the call's settings file, inside the call's fence, and cannot replace or turn off shep-kelpie's own. A hook that runs a script in the repo reaches it through the call's folder, which for the project manager, the advisor and the researcher is not the repo, so the script is not found there. Only the `hooks` are read from `settings.json`.
- `repo_setup.own = false` leaves all of this out and keeps the repo's plugins. `parts` narrows it as it narrows a plugin. A link in these folders is left out, as in a plugin.
- `status` lists it under `plugins` as `.claude`, with the skills it brought, and `doctor` reports it under `repo plugins`. A repo with none of these files has no entry.

##### MCP servers

A plugin can bring MCP servers, and so can the repo's own `.mcp.json`. shep-kelpie runs most calls with `--strict-mcp-config` and an allow-list in `--tools`, which drops every server a plugin brings even when the plugin loads. So it reads them itself, and while `mcp` is among `repo_setup.parts` (the default) adds them to the call's MCP config and their tools to its allow-list.

- A plugin's servers come from its `.mcp.json`, or the `mcpServers` of its manifest, inline or as the path of a file inside the plugin. The repo's own come from `.mcp.json` on the checkout's default branch, never a worker's branch and never the worktree's copy, so a pull request cannot add a server to its own review. `own = false` leaves the repo's `.mcp.json` out and keeps the plugins' servers, and `plugins = false` the reverse.
- A server is named as Claude Code names a plugin's: `plugin_<plugin>_<server>`, with any character outside letters, digits, `_` and `-` as `_`. The repo's keep their names. Its tools are allowed as `mcp__<name>`, every tool of the server. The code graph keeps its own name, which a repo's server cannot take.
- A plugin's server may run one of the plugin's files through `${CLAUDE_PLUGIN_ROOT}`. The plugin's copy holds only the parts that load, so the whole plugin is copied to `mcp-root` inside it, which Claude Code does not read, and the variable names that folder.
- The calls that get them are those `repo_setup.roles` names, on Claude Code. A call that only reads, the worker's vote on a ruling and a batch retro, gets none, so it stays unable to write. The same calls get the plugin's other parts, so a call that loads no MCP server today still loads none unless the table names its role.
- A server runs inside the call's fence, as any command the call runs does. The hosts a server reaches still need the call's allowed domains: `worker.allowed_domains` for the worker and the reviewers, and `researcher.allowed_domains` for the researcher. The project manager, the advisor and the issue writer reach the model and GitHub only, so a server that needs another host cannot connect there.
- `doctor` reports `repo mcp servers`: each server with a `url` and its host, and which of them `worker.allowed_domains` lacks. A server that starts a command names no host, since what the command reaches cannot be told from how it starts, so those are not checked.

#### Code graph

A project can give its workers and reviewers a graph of the code in the worktree, served by [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp). One call answers where a symbol is, what calls it and what it touches, where a worker would run a series of greps. It is off unless the project turns it on:

```toml
[app.dogs.kelpie.code_graph]
enabled = true
```

- `shep kelpie tools install` fetches the pinned release into `<kelpie home>/tools/bin`. Kelpie writes each call's MCP config itself and never runs the tool's own `install`, which writes to your agent config files.
- Before each worker turn and each Claude Code reviewer, kelpie indexes the work item's worktree into `<kelpie home>/<project>/code-graphs/<issue>`. Indexing is local CPU and spends no tokens. It runs on the call's own thread, not under the runner's lock, so a stop ends it, and it stops after five minutes. The folder goes when the work item's worktree does.
- The call gets the server through `--mcp-config` beside `--strict-mcp-config`, and `mcp__codebase-memory-mcp` in its allow-list. No other MCP server loads but those of the repo's plugins and `.mcp.json`, see [MCP servers](#mcp-servers). The server runs inside the call's sandbox, may read its binary and index, writes only its index, and reaches no host.
- Only the worker's working turns and the reviewers have it, and only on Claude Code. A retro, the project manager, the advisor and the researcher do not, and neither does a pi or Codex call.
- A project that turns it on while the server is not installed gets a MISSING `code graph` line in `doctor`. Its calls run without a graph and the runner logs why, so a missing binary or a failed index never stops a work item.
- The setting is read as the runner rereads its settings.

#### Retros

shep-kelpie runs a retro over a batch of finished work items, with the project manager's agent. It runs after every 10 work items finish, merged or closed, and when a drain starts with any finished since the last retro. A batch is at most the latest 10, so a history that built up while the retro was off is not read all at once. A project with no project manager runs none. The call is a one-off with a new session, not a turn of the project manager's standing session, so the report does not grow that session's context.

The retro reads a digest that shep-kelpie builds in code from the records it keeps, so the call spends its tokens reading, not collecting. It has one section for each work item: its title and type, the project manager's wakes and actions that named it, each ruling with its kind, how long it waited, the panel's votes and who answered, its review rounds and their findings, its fix turns after a failed check, its merges of main and conflicts, its wall time, its line from the usage ledger and the `Friction` section of its pull request's body. Wakes, review rounds, fix turns and merges of main come from `<kelpie home>/<project>/journal.jsonl`, which the runner adds to as each happens, because the state file forgets a work item once it finishes. The digest ends with the batch's totals: spend by role and kind, the wakes that changed nothing, and the rulings with how long each waited. The prompt asks for patterns across the batch first (repeated wakes, rulings that waited, findings that were stale or wrong, steps that cost more than the work), then problems in a single work item, and for a sentence and nothing else when nothing is worth your time.

A worker keeps a voice at no extra call. When something slowed it down, it adds a `## Friction` section to its pull request's body, a line or two written while its cache is warm. shep-kelpie reads it from the body when the work item ends. Workers are told of the section only while the retro is on.

The call can read and search and nothing else: it edits no file, runs no command and pushes nothing, on every harness the project manager runs on. On Claude Code that is an allow-list, `--tools Read,Glob,Grep,Skill` with no MCP server loaded, so nothing a project's settings allow adds to it. Its reply is the report, and the runner writes it to `<kelpie home>/retro-reports/unverified/<project>-batch-<time>.md`, headed with the project and the batch's issue numbers, then the agent and the session.

shep-kelpie never acts on a report, so nothing a retro suggests is applied. You pull the reports every so often, consolidate them and fix what is worth fixing; consolidating them stays out of shep-kelpie, because a retro that agents act on by themselves can loop on nonsense. A batch asks once. A retro that fails, times out (after 15 minutes), comes back empty or is cut short by a stop is logged and skipped, never retried, and it holds no slot, so no work item waits on it. It starts only when the project manager is free and not held back by the pacer or a terminal; a drain asks once and then skips it. A retro already running when a drain starts is waited on, up to its 15 minutes, and `status` counts that in the drain's ceiling. The retro's usage goes in the usage ledger as a `retro` call of the `pm` role, and `usage` totals it on a line of its own, so its cost can be read apart from the project manager's wakes. The retro is off unless the `[app.dogs.kelpie.skills]` table names it: `retro = { kind = "kelpie" }` runs the vendored `retro`, and a `path` or `plugin` runs a skill of your own. The worker used to run the retro, resuming its session once per work item. That cost about 14% of a night's spend, mostly in writing the worker's whole session back to the cache, and the worker never saw the project manager's holds, the rulings or the merges, which the batch retro reads.

### The review

Each pull request goes through a review before the merge gate, and no reviewer reads a head until CI has passed on it: the worker's push, and the head of each fix turn after it, wait for a green run before the next reviewer starts or a bot is summoned. A red run goes to the worker's CI fix turn first, counted toward `ci.fix_attempts`, and its fix comes back to the review. With `ci.block` off the review starts at once. The draft pull request needs CI to run while it is a draft, so `doctor` names a repo whose workflows all skip drafts (`ci on drafts`). A project lists its reviewers in `agents.reviewers`, in the order the review runs them, each an agent file whose `role` is `reviewer` (see [Agents](#agents)). Each listed reviewer runs once, in order, and a list changed mid-pass runs whichever listed reviewers the pass has not. A reviewer whose call fails three times in a row is passed over for the rest of the pass, and `status` lists it under `reviewers_skipped`. One that finds anything sends the worker all of its findings, at its own severity, for one fix turn, and the next reviewer reads the fix. A round of nothing goes straight to the next reviewer. A round of nits gets its fix turn too, and that fix starts nothing new: the pass goes on to the next reviewer as after any fix, and a bot that read the pull request before has its nits on the head that fix pushed left open, so a bot that reviews every push cannot loop on nits. A fix turn that pushes nothing parks on a ruling, unless the worker deferred every finding it was sent or every one was a nit, in which case the next reviewer reads the pull request as it stands: a ruling over nits would ask you about what never holds a merge. A file the review script skipped for its size is cut into hunks and reviewed again, and what those hunks find goes to the fix turn as any finding does; only the notice for a file it still could not review is reported with its round and never sent, since no fix of the worker's reviews it. A pass that ends with no reviewer having read the pull request, because one was down, kept failing or reviewed no file, marks the work item unreviewed: `status` shows why, the merge ruling's question says so, and under `auto` it gets the merge ruling instead of merging. An empty list, or reviewers whose `paths` all miss the change, is your choice, and the log says so once. Every round reads the head on `origin`, which is what CI and the merge take: before one runs, the worker's worktree must hold nothing uncommitted and have that head checked out. If it does not, the worker gets one turn to push or discard, and a worktree still off the pushed head after it parks on a `stuck` ruling for `unpushed`, naming the files, and both heads when they differ, whose yes gives the worker another such turn. After the last one the pull request goes to CI. There is no judge and no second pass: whatever the last fix leaves is what CI and the merge see. The merge takes only a head a round read, or one kelpie sent to CI unread on purpose: a fix turn's push, for review findings or red CI or a conflict, a pass your list gives nobody to read, or its own catch-up with `main` of such a head; an adopted pull request's head as it arrived counts too. Any other head, such as one pushed by hand, is named in the merge ruling, and under `auto` gets the ruling instead of merging. A pass starts again from the top only on new code that needs the whole review, such as the change a merge ruling's `rework` asks for, a change you accept that someone else pushed, or a rework of the pull request. The fix a merge ruling's `no` asks for goes to CI and back to the merge ruling, with no new pass. An empty list reviews nothing. When `ci.test_command` is set, a reviewer on Claude Code may attach a failing test to a finding (see [Agents](#agents)). Each attached file is checked as the round's findings are taken: it must be a regular file the reviewer left, not a link and at most 1 MiB, at a path the worktree does not already hold. A finding that fails the check keeps its text and loses the test, and the step report says why. When the reviewer left no such file, the named path goes back into the finding's `why`, since a pipe in the `why` can read as a path. A checked file is copied to `review-tests-<issue>` in the worker folder, which the worker cannot write, and the worker reads the original in the build folder. Kelpie runs each kept test twice and blocks nothing on either run: once in `send_findings` at the head the reviewer read, once after the fix turn when the fix moved the head and the worktree holds the pushed head, the worker's own commit. A run puts the kept copy in the worktree at its path, runs `ci.test_command` there with `{test}` and `{name}` filled in, and takes the file out again, so the worktree is as clean as it was. A file already at that path with the reviewer's exact text, the copy the worker committed with its fix, is run as it is and left; one with other text is not run over, and the run is `not-run`. While a test is in the worktree a record of it stands in the worker folder, so a kelpie stopped mid-run takes the file out when the next round starts and says so in its notes. The command runs in the same sandbox the agents use, under the worker's fence and never the maintainer's reach, holds the `cargo-test` lease while it runs, and is killed after 10 minutes. A path with characters a shell would read is not run. Each result is `passed`, `failed` (the last 2 KiB of output), `timed-out` or `not-run` (with why). Each check is kept in the work item's `test_checks`, which `status` shows with both outcomes per finding, and the fix turn's step report carries the round's. Failed then passed reads as verified. Passed before the fix means the test does not reproduce the finding. Failed after the fix is recorded as still failing, and the pass goes on. A head pushed by someone else is not run, and the report says how many tests were not. Nothing is posted on the pull request.

A finding the worker leaves as out of scope goes into its deferred findings file. Once the pull request merges, `git.issues` decides what becomes of each one above a nit, whatever `git.merging` is: under `ask` a `follow-up` ruling comes first and a yes files each as a `ready-for-agent` issue linking the pull request, onto the board, since your yes is its triage. Under `file` they are filed with no ruling as `needs-triage`, which keeps them off the board until you read them and label them `ready-for-agent`; a repo without `needs-triage` gets it before the first one is filed. The one `follow-up` ruling `file` can raise comes when the forge has refused to take them for six hours: a yes there retries the filing, and they still go out `needs-triage`. Under `skip` nothing is filed and they stay in the review. A nit left there is dropped.

A reviewer runs in one of four ways, by its file's `harness`:

- `claude-code`, `codex` or `pi`: a fresh session on its `model` and `effort`, which reads the worktree with Read, Grep and Glob and runs no command. The file's body is its prompt.
- `command`: a command of your own that keeps the contract below, and takes no body.
- `endpoint`: shep-kelpie's own reviewer, for any OpenAI-compatible server such as Ollama, LM Studio or llama.cpp's server. Its body is optional. With none, kelpie sends its own prompt. With one, the body replaces kelpie's opening line and its list of what to report, so it can say what the project cares about and what counts as a finding. Kelpie always adds, after the body, how the chunked, line-numbered diff looks and the answer format (`SEVERITY|file:line|what|why`, or `CLEAN` alone), then the issue's acceptance criteria. A body that asks for another answer format is still read in kelpie's. Kelpie cuts the diff into chunks, so a body naming `{{BASE}}` or `{{DIFF}}` is refused.
- `bot`: a pull request review bot, CodeRabbit, cubic or Codex, summoned on the pull request (see [Review bots](#review-bots)), and takes no body.

A session's prompt is its file's body with the commit the change is against at `{{BASE}}` and the diff at `{{DIFF}}`. A body with no `{{DIFF}}` gets the diff after it. The body asks for kelpie's format, one finding per line as `SEVERITY|file:line|what|why`, or `CLEAN` alone. Every reviewer's prompt carries the issue's acceptance criteria: the section under an "Acceptance criteria" heading, or the whole body without one. `paths` limits a reviewer to pull requests that change a file under one of its globs, and the review skips it elsewhere. `second_look: true` runs a session twice: the second fresh session is shown what the first found and asked only for what it missed, and the worker gets one fix turn for both lists. With `ci.test_command` set, a `claude-code` reviewer also has `Write`, held to one folder, `review-tests` in the work item's build folder, and is asked to leave one failing test per finding there, at the path the test should have in the repo, named as a fifth field: `SEVERITY|file:line|what|why|tests/flag.rs`. A fifth field that is not a relative path with no `..` and no spaces stays part of `why`, and a reviewer of any other kind, or any reviewer without the setting, is read as before.

shep-kelpie ships `defect-hunter`: Opus 5.5 at high effort reading the whole pull request for defects, meaning a trigger and an effect a failing test could be written from, and not for style or naming, with a second look. It is a project's review when the project lists none. `qwen` runs `~/.claude/scripts/qwen-review.sh`, an optional local review script, and when that script exists a project that lists none runs `qwen` first.

`standards` and `spec` are the two axes of the vendored `code-review` skill, one reviewer each, since the skill runs them as parallel sub-agents and kelpie's workers spawn none. Both run Opus 5.5 at high effort, read once with no second look, and answer in the same `SEVERITY|file:line|what|why` lines. They do not replace `defect-hunter`. `add` writes both files and lists neither, and a project's `agents.reviewers` list replaces the default, so it runs them by naming them in that list, `defect-hunter` included: `reviewers = ["defect-hunter", "standards", "spec"]`.

- `standards` reads `CODING_STANDARDS.md`, `CONTRIBUTING.md` and `CLAUDE.md` where the repo has them, and checks the diff against them and against the skill's twelve code smells, such as Duplicated Code and Speculative Generality. It covers the verbose and repeated code `defect-hunter` is told to skip. A documented standard overrides a smell, what tooling enforces is skipped, and a smell is never above `LOW`, since each is a judgement call.
- `spec` checks the diff against the issue's acceptance criteria, which are cut at 4,000 bytes. It reports a requirement that is missing or partly done, behaviour nobody asked for, and a requirement that looks done but is wrong, quoting the criterion for each.

A local model before the defect hunter, and a deeper read of the paths where it pays:

```toml
[app.dogs.kelpie.agents]
reviewers = ["gpu-box", "defect-hunter", "merge-reader"]
```

```markdown
---
role: reviewer
harness: endpoint
url: http://localhost:11434/v1
model: qwen2.5-coder:14b
context: 32768
---
```

```markdown
---
role: reviewer
harness: claude-code
model: claude-opus-5-5
effort: high
paths: ["src/auth/**", "migrations/**"]
---

Read the change against {{BASE}} for ways it lets the wrong account in. Output
one finding per line as SEVERITY|file:line|what|why, or exactly CLEAN.
```

A missing command or an endpoint that doesn't answer stops the runner at start.

#### Review bots

A review bot reads the pull request on the forge, in its place in `agents.reviewers`, once a pass. shep-kelpie ships a file for each, `coderabbit`, `cubic` and `codex`, which `add` writes out and no project lists until you list it:

```toml
[app.dogs.kelpie.agents]
reviewers = ["defect-hunter", "coderabbit"]
```

```markdown
---
role: reviewer
harness: bot
bot: coderabbit
reviews: 1
hours: 1
---
```

A bot's file is named for its bot, since `reviews` in `hours` is its account's window, which the dog books from that file when it starts and every project shares. Its round marks a draft ready, since a bot may skip drafts, waits for the bot's window and lease, and summons it: CodeRabbit by the `review please` label, or by `@coderabbitai full review` on a pull request it read before that would otherwise find nothing new, cubic and Codex by their comments. A summon with no sign of being heard in fifteen minutes goes out once more. Once a review covers the head, the bot's open threads are its findings, and go to one fix turn as any reviewer's do. A refusal hands the window back to the dog and the round asks again. A window that opens more than an hour on, by the dog's book or by the bot's refusal, passes the bot over for the pass, and so do two hours with no review, a status that declines the summon (CodeRabbit's `Review skipped: automatic reviews are disabled` or `Review skipped: excluded by label configuration`, which its `.coderabbit.yaml` sets and passes the bot over at once), two hours its round could not summon it, a bot you stop listing mid-round, and CodeRabbit on a repo the forge reports not public, which is checked just before each summon; `status` lists each under `bots_skipped`, and a pass nobody else read is marked unreviewed. A bot passed over that reviews the head anyway is still read: at the start of each later round, when CI goes green, and every five minutes while the merge ruling waits. Its review gets a round of its own, and its threads go to one fix turn as any review's. Once the pass has ended, that fix goes back to the merge ruling with no new pass, after the round of any other passed-over bot that has reviewed late too, and the ruling says no reviewer read it and comes to you even under `auto`. A merge ruling already waiting is withdrawn for it, which the log and the webhook say, and the work item takes a free slot for the round or waits for one. A bot that already read the pull request this pass and reviews a later head is not sent again: its open threads are named in the merge ruling, and under `auto` any above a nit hold the merge. Open nits are named in the merge ruling on their own, as in `3 nits left open (cubic)`, and never hold a merge. `rounds: 1` lets a bot read a work item's pull request once, whatever its passes, counting its reviews from before a rework or an adoption, and is unset by default. An adopted pull request goes through a pass of the listed bots alone, and until each bot answers a summon of shep-kelpie's own, none of its reviews from before the adoption stands for its read. Codex's `reviews_on_ready: true` says it reviews a pull request when it leaves draft, as the repo's Codex settings may have it: then marking ready, under its lease, is the summon, with no comment, and it is never asked again by comment. List it before any other bot, whose mark-ready would draw its review outside its lease: the runner refuses the other order.

No review bot is listed unless you list it, and a project that lists none never asks the forge about one. CodeRabbit's free plan reviews public repos only, so a repo GitHub marks private cannot list it, and lists `cubic` or `codex` instead. shep-kelpie asks CodeRabbit for a review by putting the `review please` label on, so the repo's `.coderabbit.yaml` must review only labelled pull requests. Without that, CodeRabbit reviews every push on its own and spends the hour a round is waiting on:

```yaml
reviews:
  auto_review:
    labels:
      - "review please"
```

Every thread a review bot leaves open goes to the worker as a finding. The fix turn's prompt asks the worker to end its reply with one line per finding, numbered as its line in the findings file is (`thread 1|MEDIUM|...`): `thread 1: fixed`, or `thread 2: declined: <the reason>`. shep-kelpie, not the worker, writes to the pull request: it replies on each thread and resolves the ones the worker fixed once the fix moves the head. A declined thread gets the worker's reason as a reply and stays open. A thread the reply does not mention gets a reply saying it was not addressed and stays open, and so does a thread the worker calls fixed in a turn that pushed nothing. Open threads are still the merge ruling's to name, so a declined or unaddressed thread is counted there. This holds for every fix turn a bot's threads go to: a review pass's rounds, a late round, and the new pass a merge ruling's `no <note>` starts when the ruling warns of open threads. A nit fix that pushes nothing (the worker declined the nits) gets the same replies. If the forge refuses a reply or a resolve three steps in a row, the review goes on with the rest open and the bot's next read sends them again.

#### Local reviewers

An endpoint takes `url` (the base, up to and including `/v1`) or a `gateway` (see [Gateways](#gateways)), `model`, and `context`, the context size in tokens the server gives that model. Kelpie diffs the pull request, cuts the diff to fit that context, and sends each piece with its own review prompt. Set `context` to what the server really uses: Ollama gives its OpenAI-compatible endpoint a small default context unless `OLLAMA_CONTEXT_LENGTH` says more, and drops whatever doesn't fit without saying so.

A command is run as `<command> --dir <worktree> --round <n> --diff <base>`, where `<base>` is the commit the branch started from (the merge base of the project's base and the head), so a diff of it against the worktree holds the pull request's own change, with:

- `QWEN_REVIEW_OUT`: the folder to write in
- `KELPIE_REVIEW_HEAD`: the commit under review
- `TMPDIR`: the temporary folder the maintainer's shell has (macOS's per-user folder when the runner has none), where the script builds its own GPU lock
- `KELPIE_REVIEW_CRITERIA`: a file holding the issue's acceptance criteria

It writes `round-<n>.txt` in that folder, one finding per line as `SEVERITY|path:line|what|why` with `HIGH`, `MEDIUM` or `LOW`, and then an empty `round-<n>.txt.done`. Kelpie reads nothing without the marker, and nothing from stdout. A nonzero exit fails the round. A command that writes `LOW|<path>:0|not reviewed: <n> lines exceeds the chunk limit|...` is run again with `--files <hunk file>` in place of `--diff`, on that file alone. If that run fails, its file is left unreviewed, as below, with the failure as the reason; only when kelpie cannot cut the hunk with `git diff` does the placeholder stay as the finding. Any other `LOW|<path>:0|not reviewed: <why>|...` line, as the script writes when the model cannot be reached, is a file left unreviewed and not a finding. A round with nothing but those lines reviewed nothing, and the review goes on to the next reviewer. A round that leaves the same files unreviewed as the same reviewer's last round counts against it too, and one that leaves none clears its count. A second such round in a row, which takes two passes, leaves the reviewer out of the review for the rest of the work item, and `status` lists it under `local_reviewers_down`. Another local reviewer, on another command or server, still runs. A round with real findings and some `not reviewed:` lines keeps its findings.

Kelpie holds no lock around a round, and a `lease` key in an agent file is refused. A server shared with other callers goes behind a `gateway`, which queues its rounds. A command that needs the GPU to itself takes its turn from paddock, with `shep paddock run --model <model> -- <command>`.

Kelpie reads no `/api/ps`: a model that spilled to the CPU is no longer checked or parked on a ruling.

### CI and the merge

After the review's last reviewer, kelpie waits for CI on the pull request's head. A red run is the worker's next turn, naming the failed checks; a second red run on a head the worker left alone asks you. Before it spends that turn, kelpie reads the end of the failed runs' logs with your `gh` login, and when none of them names a file the branch changed (a panic location, a `--> path` line, a compiler error) it reruns that head's failed jobs once with `gh run rerun --failed`, since the worker's credential cannot, and sends no turn. `status` shows the head under `rerunning` while the rerun runs, and the log has a `ci-rerun` step. A head that fails again after its rerun goes to the worker as any red run does, and the prompt says the failure repeated, so the worker does not call it a flake. A log kelpie cannot read, one that names a file the branch changed, a rerun the forge refuses, and a head already rerun all go to the worker with no rerun. A rerun costs no fix turn, so `ci.fix_attempts` does not count it. `ci.fix_attempts` caps those fix turns for a work item: once the worker has had that many, the next red run asks you instead, and `0` asks on the first. `-1`, the default, sets no cap. A green run on a branch without the latest `main` is caught up first: kelpie rebases its own commits onto `main` and pushes with a lease, or merges `main` in where the branch holds a merge or commits that are not its own, and a conflict is the worker's next turn. A project with `ci.block = false` reads no checks, and `ci.fix_attempts` does not apply. With it on, no reviewer reads a head until CI has passed on it (see [The review](#the-review)); with it off, the review starts at once.

A project on `git.merging = "ask"` gets a ruling before every merge, and a yes merges only the head it was asked about, while CI on it is green and it has the latest `main`. A project on `"auto"` merges its pull requests without asking once every gate passes, and posts a notice after. Every other ruling still asks under `auto`, and so does the merge for a pull request no reviewer read, one with a review bot's thread above a nit open, a head no round read, the fix a merge ruling's `no` asked for, and the fix for a bot's late round: each is a safety gate, not a setting, and the ruling names which one raised it. The merge is always a merge commit of the head the gates passed, never a squash, and kelpie then removes the worktree, both branches and the build folder. With a merge queue on `main`, kelpie queues the pull request and waits on the queue.

A commit someone else pushes to a work item's branch, or a label or ready state kelpie did not set, parks it on a `foreign-change` ruling. A pull request that changes agents' own files, such as `.claude` or `.mcp.json`, parks it on an `agent-files` ruling. A pull request you merge by hand ends its work item; one closed without merging parks it. A work item already parked on a ruling reads its pull request at most once every five minutes: a merge ends the item and withdraws its ruling, and a close replaces its ruling with a `closed` one, except for the follow-up ruling after kelpie's own merge, which is not read. An item parked on that `closed` ruling is read on, and a pull request reopened and merged ends it.

## Agents

An agent is a file: `$SHEP_HOME/kelpie/agents/<name>.md` (or the `agents` folder of the home `KELPIE_HOME` names), YAML frontmatter and then a Markdown body. The name is the file's name without `.md`. An implementer's body is added to kelpie's own instructions for that agent, and an empty body adds nothing. A reviewer's body is its prompt (see [The review](#the-review)).

```markdown
---
role: implementer
harness: claude-code
model: claude-sonnet-5-5
effort: high
---

Extra instructions for this agent, added to kelpie's own.
```

`role` is what the agent is for: `implementer`, an agent that builds a work item, `reviewer`, one that reads a pull request, a review bot included, `issue-writer`, the one `shep kelpie issue` runs, whose body is its prompt and which runs on `claude-code` alone (see [Writing issues](#writing-issues)), or `pm`, the project manager, likewise (see [The project manager](#the-project-manager)), or `advisor` and `researcher`, the two experts besides the worker and the project manager who vote on each ruling, whose body is their prompt too and which run on any harness (see [The panel](#the-panel)). A key only another role takes, such as a reviewer's `paths` or `second_look`, stops the runner. `harness` is Claude Code, `claude-code`, pi, `pi`, which runs a model on an OpenAI-compatible server such as Ollama, or Codex, `codex`, on a ChatGPT plan. `model` and `effort` are what the harness runs. `tools`, on a `claude-code` implementer or reviewer, is the list `claude --tools` takes, in place of what its role starts with: `Bash,Read,Edit,Write,Glob,Grep,Skill` for an implementer, and `Read,Glob,Grep` for a reviewer. Name it for a project whose workers need more, such as `WebFetch`; the settings file still denies the tools that reach past the worker's fence. `extends: <name>` builds a file on another agent: it starts from that agent's keys and body, the keys it sets replace the parent's (`role` and `harness` too), and a file with no body takes the parent's. The parent is any agent file in the folder or one of kelpie's own agents, and it may extend another. A lighter `defect-hunter` is a file holding `extends: defect-hunter`, `effort: medium` and `second_look: false`. The merged file is checked as any file is, and a refusal names the child. A missing parent or a cycle stops the runner, naming the file. The variant keeps its own name for its `agent:` label, ledger lines and pacing, and an edit to the parent reaches it at the next wake. Any other key stops the runner, and so does a file that does not parse or misses a key its harness needs, naming the file and the key. A `.md` file whose name is no agent's, such as a `README.md`, is skipped and named in the log. A runner sees an edit to the folder the next time it wakes, as it sees a settings change.

Kelpie ships `sonnet-high`, Sonnet 5.5 at high, the default implementer, `opus-high`, Opus 5.5 at high, for work that is hard to undo, `defect-hunter`, the default reviewer, `standards` and `spec`, the two axes of the `code-review` skill, each a reviewer that reads once (see below), `qwen`, the reviewer that runs the qwen-review script, the review bots `coderabbit`, `cubic` and `codex`, `issue-writer`, Opus 5.5 at medium, `pm`, the project manager, Opus 5.5 at medium, `advisor`, Opus 5.5 at high, and `researcher`, Sonnet 5.5 at high. `shep kelpie add` writes out any that are missing, `qwen` only where the script exists, and never writes over one you edited, and a file named for one replaces it.

A project lists the agents that build its work items and the ones that review its pull requests, from those files:

```toml
[app.dogs.kelpie.agents]
implementers = ["sonnet-high", "opus-high"]
reviewers = ["defect-hunter"]
```

An issue labelled `agent:<name>` (the prefix in any case) runs on that agent, which the project must list. With more than one listed, the issue writer labels any other issue before the board opens it; with one, it runs on that one. Any implementer may be first, a local one included, and a list may be all local. `sonnet-high` alone when absent. An implementer's file must say `role: implementer`, and a reviewer's `role: reviewer`. When the runner starts, and when `agents.implementers` changes, it makes each listed implementer's `agent:` label the repo lacks. It never removes or changes a label, since removing one strips it from every issue, closed ones included.

A work item keeps the agent it opened on, and each turn runs that agent's file as it is then, with one exception. With `agents.fallback_after` set, in minutes, a work item's first turn that has waited that long for its model is stopped and moves to the next implementer listed after its own that no other work item is waiting on or moving to. Its first turn has no output and no session yet, so nothing is lost, and its next turn starts fresh on the new agent. The log's `fell-back` line names both agents. A later turn never moves, nor does an item whose label ends in `!`. With no implementer after its own free, it goes on waiting.

A turn waits for its model while it asks for a lease on it, from a gateway, while its gateway answers busy, or while it has shown no output for `agents.fallback_after` (10 minutes with that off). `status` shows it on the work item as `waiting`, with `since` and `why` (`lease`, `busy`, `silent` or `unreachable`), and the board as "Waiting for its model since". Its first output clears it. A turn whose model host cannot be reached, a pi call that got a 502 or 503 from it or had its connection refused or reset, is not parked on a ruling. It keeps its session and is tried again after 30 seconds, then after twice as long each time up to 5 minutes, and `status` shows `unreachable` from the first failure. Past 30 minutes since that failure it parks on a `turn-failed` ruling that carries the last error. The 30 minutes is not a setting, and `agents.fallback_after` never moves an item for this wait. A restart begins a new wait, since the retry is held in memory only. That covers a turn behind a gateway's lease, and a harness whose output kelpie reads as it comes, such as pi. A Claude Code turn is not reliably marked: headless Claude Code retries a provider's overloaded or rate-limit answer inside the call and says so only when it ends, and kelpie reads its activity from the session's transcript, which can hold lines before any output. So a Claude Code turn may never be marked waiting, and its fallback is unreliable.

`usage` says how an agent's usage is read, and so which account paces it: `claude` (the default on Claude Code) reads `/usage`, `codex` reads Codex's own 5-hour and weekly windows, and `none` is a local model that is never paced. It must be the harness's own reader, so leave it out: Claude Code reads `claude`, pi reads `none` and Codex reads `codex`.

A pi agent also needs the model's server and the context size it gives the model:

```markdown
---
role: implementer
harness: pi
model: qwen3.8:27b
effort: medium
url: http://<host>:11434/v1
context: 65536
---
```

Kelpie runs pi with a home of its own, so your `~/.pi` is never read. pi runs no Claude Code hooks, so a worker on pi can't have `worker.guard_hooks`, and the runner refuses to start with them. Kelpie's own checks still run on every command and file write, and so do your `hooks`.

A pi call's sandbox allows no host of the model's, since the sandbox opens every port of an allowed host and Ollama's admin calls (pull, delete, create) answer beside chat. Kelpie runs a forwarder outside the sandbox for each call, and the sandbox allows only that. It passes `POST` to the server's `/v1/chat/completions` and refuses every other path and method, naming what was asked. The worker never sees the model's address. The forwarder dials `http://`, so `url` is the server's `http://` address: an `https://` one is refused when settings load.

#### Gateways

A gateway such as paddock, a shep dog, puts one host's model servers behind one endpoint: it loads and unloads models, queues what doesn't fit yet, and lets a long job hold a lease on its model. Name it once in shep-kelpie's own settings:

```toml
[kelpie.gateways.paddock]
kind = "paddock"
url = "http://gpu-box:8700"
key_env = "PADDOCK_KEY"
```

`kind` says what the gateway is, which decides how kelpie checks its key and takes a lease on a model. It is `paddock` (the default when `kind` is left out) or `openai`. An unknown kind is refused naming it. `url` is its `http://` address, with its OpenAI routes under `/v1`. `key_env` names the variable in the runner's environment that holds the key the gateway gave kelpie, so set it in the runner sheep's `env`.

Any OpenAI-compatible gateway, such as LiteLLM, OpenRouter or a bare llama.cpp server, takes `kind = "openai"`:

```toml
[kelpie.gateways.litellm]
kind = "openai"
url = "http://gpu-box:4000"
key_env = "LITELLM_KEY"
```

An `openai` gateway knows nothing of paddock's routes. Kelpie lists its models from `GET /v1/models`, checks the key with that same route (`200` is taken, `401` or `403` refused, any other status unsure), and asks it for nothing under `/paddock/`. It holds no lease, so a call on it starts at once, and the gateway may evict a model a turn is running on.

A runner reads its gateways when it starts. A `pi` agent or an `endpoint` reviewer then names the gateway in place of `url`, with `model` as the gateway knows it:

```markdown
---
role: implementer
harness: pi
model: qwen3.8:27b
effort: medium
gateway: paddock
context: 65536
---
```

For a model behind a gateway:

- Kelpie reads no `/api/ps`. The gateway queues each call and decides what stays loaded, and kelpie takes the gateway's lease for each call itself.
- The key never enters the sandbox. pi's provider file holds a placeholder, and the forwarder sends the gateway's key in its place. An endpoint's round sends it from kelpie's own `curl`, on its stdin. Every process kelpie starts, an agent call or not, has `key_env`'s variable unset, and a reviewer's session can't read the shepherd's home, where the runner's `env` keeps it.
- A runner won't start while an agent it lists names a gateway its settings lack, or one whose key's variable is unset.
- On a `paddock` gateway, each call holds a lease on its model from its start to its end: a worker's turn, a reviewer's session, and an endpoint reviewer's round. The gateway evicts nothing it runs on midway. The ceiling counts from the grant, and a gateway too busy to grant it is asked again every 30 seconds until then. A call that ends or fails releases it, and a runner that dies stops renewing it, so it runs out two minutes later. A gateway that answers that the model is serving another call (`reason_kind: turn`) is a wait for a turn, shown as `waiting` for a worker and as a queued round for a local reviewer.
- Every chat call kelpie sends a gateway asks for batch priority (`X-Paddock-Priority: batch`), whatever its role, as does an endpoint reviewer's round. Paddock serves a request with no priority as interactive and serves interactive requests first, and kelpie runs unattended, so none of its calls sits ahead of your own. A worker's own `X-Paddock-*` headers never reach the gateway, a model on a plain `url` gets none, and a call made while kelpie holds turn-taking leases on the same model still uses one of those lease turns.
- A pi call's tokens reach the usage ledger unpriced, as any pi call's do. The gateway doesn't say how long a request queued, so that wait is part of the call's seconds.
- `shep kelpie doctor` checks each gateway: that it answers `GET /v1/models` (with the key, for an `openai` gateway), that it takes the key (unsure when the shell you run doctor from doesn't have the variable), and that it lists every agent's model.

An implementer behind a gateway is a local implementer, like any `usage: none` one. Claude Code can't run on a gateway yet. A gateway serves as many conversations at once as it allows. A project sharing one local model server without a gateway keeps `concurrency.active_items` at 1: two work items on one model that serves one sequence at a time evict each other's cached prompt, and each request reads its whole prompt again.

A Codex agent needs the `codex` command on the shepherd's `PATH` and a ChatGPT plan, and takes only `model` and `effort`:

```markdown
---
role: implementer
harness: codex
model: gpt-6.1-sol
effort: medium
---
```

`codex debug models` lists the plan's models. It runs on shep-kelpie's own ChatGPT login, in `codex_home` (`$SHEP_HOME/kelpie/codex` by default, or the path `codex_home` in the `[kelpie]` section names), never your `~/.codex`. Sign it in once:

```sh
CODEX_HOME="${SHEP_HOME:-$HOME/.shep}/kelpie/codex" codex login --device-auth
```

Each Codex call gets a home of its own with that login linked in, so no call sees another's sessions. A runner reads `codex_home` when it starts. A worker on Codex can't have `worker.guard_hooks`, the same as pi. Kelpie's checks run as Codex's own hooks: `confine` on every `apply_patch`, `guard` on every command.

A local implementer is one whose usage is `none`. Each account keeps its own daily allowance and 5-hour stop, shown under its name in `status.pacer`, and a new work item waits on every account its roles spend. `shep kelpie doctor` checks Codex answers for a project that spends it. A `none` agent is never paced, and kelpie queues nothing for it: a model behind a gateway waits in the gateway's queue, and `status.local_leases` shows the holder of the gateway's lease on it.

`status` shows each role's tokens in `by_role`, with `cost_usd` only for calls whose harness reports dollars. `unpriced_calls` counts the rest.

## The board and the project manager

### The briefing

The runner writes the board out as `board.md` in the project's folder, and `status` names the file under `board`. It is the briefing the project manager's agent reads, so that agent never runs git or gh itself, and you can read it too. Code writes it, with no model call, at start and whenever the board changes: a call starting or ending, a phase change, a ruling raised or answered, a read of the ready queue. It is written again at least once a minute while the runner looks at the board, and each write replaces the file whole. Text that names this machine is withheld from it, as from the forge. It holds:

- each open work item: its issue, phase, pull request, age and agent, the worker's last closing message, and the files its branch touches
- what each item's session is doing. A worker's turn shows when it last made a tool call or wrote output, read from its transcript (Codex's output file, pi's session file), and reads as idle after 10 minutes of neither
- the rulings waiting on you
- the ready queue in the board's order (the order the board keeps, first taken first), with each issue's priority, why the board passes over it if it does, the paths its body names in backticks, and the first 600 characters of its body, quoted
- the board's events since the project manager last read it, or the last 20 before it ever has
- the overlap: for each pair of open branches, the files both touch and the files `git merge-tree` finds in conflict, and each ready issue's named paths against them. The conflict check needs git 2.38 or later

### The project manager

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

Each wake carries every reason gathered since the last one, in one call that runs as a lamb like any other and that the runner never waits on, one at a time per project. It runs in `<project>/pm`, which holds `board.md` as it stood when it woke, each open work item's diff from `main` as `diffs/<issue>.diff` (cut at 64 KiB), the end of the failed run's log as `ci/<issue>.log` for an item stuck on red CI, and its own notes, `pm-notes.md`. Of kelpie's and the shepherd's homes it reads that folder and the project's worktrees alone, and it reads your checkout, all read only, so it judges a merge or a stuck item on the code. It never reads `gh`'s token, the credentials no worker reads, or a secret-shaped file in the checkout or a worktree: the checkout's `.git/config`, and any `.env`, `.env.*`, `*.pem`, `*.key`, `.npmrc` or `.netrc`. It reaches no host but the model's, has only Read, Glob, Grep, Edit and Write and no MCP server but the repo's own (see [MCP servers](#mcp-servers)), runs no command, git or gh, and may only add to the end of its notes, at most 64 KiB a write, which a hook holds it to. It answers in one JSON object: a pick, the ready issues to hold until an open work item closes, an unstick (`retry`, `re-scope`, `ask` or `none`) and a reply to you.

Kelpie checks the answer against the board before it acts. A pick or a hold must name a ready issue the board shows, and an unstick a work item that is stuck now. Anything else is dropped and logged as `pm-answered`, with why. A `retry` answers the item's ruling yes (for CI still red, sends the worker back to it), or ends an idle worker's call and resumes its session. A `re-scope` or `ask` adds the project manager's words to the item's ruling, on one line after `PM says:`, and posts it to you again. A pick of none starts nothing until its next wake; with no work item open it lasts 30 minutes before the board's rule picks, and a hold over every issue the board could start goes at once, so a project with nothing in flight never stalls. Its reply shows in `status` under `pm.reply` and in the log.

It keeps one session per project, resumed on each wake. Once a wake leaves its context past 100k tokens, the session is compacted before the next wake. A session that cannot be resumed starts again from the board. `shep kelpie pm` opens that session in your terminal, in its settings and sandbox, as `attach` opens a worker's: the runner holds the project manager for that command and for the session it starts, each known by its pid and start time, so no wake starts until both have ended, and a wake already in flight runs to its end first. One terminal holds it at a time. When there is no session yet, `pm` starts the one its wakes then resume.

With no project manager named, while it is down (a failed call passes it over for 10 minutes), over Claude's 5-hour pace, held in your terminal, or when its pick is dropped, the board's rule picks and a stuck item waits on its ruling, as without one.

Measured on 33 real decision points from this repo and shep, a project manager briefed this way avoided 9 to 12 of 11 to 15 avoidable conflicts where the board's rule avoided 4 of 15, and decided as well as one that ran git and gh itself at about half the cost a wake. On a scripted day of 40 wakes, one session compacted as it went cost $3.31 a day on Opus, against $8.16 for a fresh session every wake, and kept what it knew.

## Attaching to a worker

`shep kelpie attach <issue>` takes over a work item's worker by hand. The runner holds the work item, so no call starts for it. A call already in flight runs to its end, and `attach` says it is waiting. Then it resumes the worker's session with `claude --resume` in your terminal, in the item's worktree, with the worker's settings file and inside the worker's sandbox, so its deny rules, its guard and its fence hold while you drive. Claude Code asks before each tool call as it always does.

When you exit `claude`, the work item carries on, and its next turn resumes the same session. A commit or push you make while attached is the worker's own, as a push in a turn is, so the gate takes it as the branch's head rather than a change kelpie did not make. The runner holds the work item while either the `attach` command or the session it started runs, each known by its pid and start time, and lets it go at its next pass once both have ended. A SIGTERM or SIGHUP to `attach` is passed on to the session, which `attach` waits for. `status` shows the hold under the item's `attached`, and `drop` refuses the item while it is held.

`attach` changes nothing and says what to do instead for a work item whose worker has no session yet (its first turn starts one), one parked on a ruling (answer it first), one holding no slot under `concurrency.active_items` when none is free (its session would be one more model call; with one free, `attach` takes it), or one merging. Only a Claude Code worker can be attached: a Codex or pi worker's session resumes another way, by hand.

## Rulings

A ruling is a decision only you make. The worker waiting on one is parked, and the rest of the project goes on: a parked work item gives its slot up, so the board can open another (see [The board](#the-board)). It is one of six kinds:

- `merge`: CI is green, and a yes merges the pull request. A `no <note>` sends the worker your note, and its fix goes to CI and back to you with no new pass of the review; a `rework <note>` sends your note for a change the whole review reads again. On a merge ruling that warns of open review bot threads, a pass no reviewer read or a head nothing vouches for, `no` starts a new pass too, since only a pass clears that, and the question says so. The merge ruling after a `no` fix, or after the fix for a review bot's late review (see [Review bots](#review-bots)), says its head is that fix and that no reviewer read it, and comes back to you even under `auto`. Open review bot nits are named on their own and hold nothing
- `question`: the worker asks, and your answer is its next turn
- `stuck`: the work item cannot go on by itself, and its reason says why: `rebase`, `still-red`, `merge-refused`, `closed`, `fix-not-pushed`, `unpushed`, `turn-timeout`, `turn-failed` or `over-budget`. The question says what a yes does
- `agent-files`: the pull request changes agents' own files, and a yes accepts them
- `foreign-change`: someone else changed the pull request, and a yes accepts the change
- `follow-up`: a merged pull request left findings unfixed, and a yes files them as issues

A ruling shows in the runner's log, in `shep kelpie status` and in `shep kelpie rule`, and posts to the webhook when kelpie's `[kelpie.webhook]` is set (see [Settings](#settings)). The pull request gets a comment for every ruling but a merge. Answer it from the terminal:

- `shep kelpie rule 14 yes`
- `shep kelpie rule 14 no rename the flag`
- `shep kelpie rule 14 rework split the parser out`, for a merge ruling only
- `shep kelpie rule 15 use --dry-run`, for a worker's question. A `yes` there is the answer's text

A ruling's id is unique across projects, so `rule` needs no project. You can name a ruling by the number of its pull request or its issue instead: `rule 427 yes` and `rule 407 yes` answer the same ruling when 427 is the pull request and 407 its issue. A work item waits on one ruling at a time, and GitHub numbers issues and pull requests from one sequence, so within a project a number names one ruling. `rule #427 yes` always means a number, and a bare `427` is a ruling's id when one is waiting under it. From a clone, the clone's project is looked at first. Elsewhere a number that one project has a ruling on works, and one that two projects have is refused with their names: `rule shep-kelpie#427 yes` picks one. Quotes are optional, but zsh still needs them around a note with `?`, `*`, `!` or an apostrophe. `-p` goes before the answer: after its first word, and after a `--`, a `-p` is part of the answer. `shep kelpie rule` alone lists the rulings waiting and asks which to answer and how. Each ruling's question says what `yes` and `no`, and on a merge ruling `rework`, do. A `no` or `rework` with a note is the worker's next turn.

Where an App covers the repo, a ruling's comment is one short format, whatever its kind: the problem where there is one, a title, a table of what picking each option leads to, a Panel list, and the ways to answer. The Panel lists each expert's vote once the panel has voted (see [The panel](#the-panel)), and holds the project manager's words where it gave any until then, or when the project has no panel. Here is a merge ruling. Kelpie writes the effect of each option from what it knows: the items held behind the pull request, CI, the reviewers that read it, and roughly what another review round costs from the ledger:

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

On an ntfy webhook, a ruling can be answered from the topic: run `shep kelpie totp` once and scan the QR code into an authenticator app, then reply with the line the alert ends on, such as `14 yes <code>`, with the app's code last. A reply takes the same answers as `shep kelpie rule`, and names the ruling the same ways: `427 yes <code>`, `#427 yes <code>` or `koji#427 yes <code>`. A reply by number written before the open ruling was raised is refused, saying the ruling changed and what it asks now, since a new ruling on the same pull request may ask something else. A reply by id, and an answer at the terminal, are taken as given. Anyone who can read the topic can read rulings, but only a reply with the code of the moment answers one, and each code answers once. Five wrong codes turn answers off until `shep kelpie totp --unlock`, and `shep kelpie totp --rotate` replaces a secret that may have leaked.

With a project manager, it may answer a `stuck` ruling for you with a retry, or add its words to the ruling and post it again (see [The project manager](#the-project-manager)).

### The panel

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

#### Deciding

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

#### Who votes

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

The kinds are `question`, `agent-files`, `foreign-change`, `follow-up`, `rebase`, `still-red`, `merge-refused`, `closed`, `fix-not-pushed`, `unpushed`, `turn-timeout`, `turn-failed`, `unlabelled` and `over-budget`. `merge` is refused: the runner does not start, naming `rulings.pm_alone`, and a change that adds it while the runner runs is refused too, keeping the settings it has. A question, and a ruling on a risky path, are all four's even when their kind is listed, so `question` under `pm_alone` only fills its `pm_alone` run in `status`. A `design` flag from the project manager calls the full panel, and an abstention the panel the item's type sets, or all four for a question or a risky path. Under `shadow` every expert still votes, and `status` reports, for each kind of ruling, the run of rulings where the project manager's vote was your answer (`pm_alone`), the way to see which kinds to list. A vote that flagged `design` or abstained leaves a kind's run as it is.

An expert's agent can be off the panel: an advisor or researcher the project does not name is never asked, and counts as an abstention, so a project without a researcher decides a `feat` on three of three.

#### Measuring

The panel costs up to four agent calls on every ruling, one of them Opus at high effort, so each expert has to show it is worth its cost.

- Every vote is a line in the usage ledger (`usage.jsonl`) with `votes`: the ruling's id and kind, the expert, the panel the item's type set (`pm`, `pm+advisor` or `all`), the type, and its choice, left out for an abstention. The line holds the call's tokens, cost and wall time as any call's does. The project manager's wake is counted the same way: its line has a vote for each ruling it voted on, and shares its cost between them.
- `shep kelpie usage` splits the panel's votes by expert, by kind of ruling and by type: calls, cost, median wall time, abstentions, and the cost per ruling. A wake that also served another reason counts in full.
- `status` reports, as `panel`, each expert's votes, abstentions, `agreed` (votes that were your answer) and `lone` (times it alone was right, such as the one vote for a `no` you then gave), and what each smaller panel, asked alone, would have decided against your answers (`panels`: how many, and how many as you answered). It comes from `rulings.jsonl`, which holds the rulings answered from this build on. An expert that is never the only one right adds cost and no decisions. Each expert's `agreed` and `lone` are also split by kind of ruling (`by_kind`). Both count only your answers, not the project manager's or the panel's.
- `status` also reports, as `whatif`, how seven candidate policies for who votes would have done over the rulings answered under `shadow`, where every expert votes blind so any policy can be played over the same ballots with no extra call. The candidates are fixed in code and are not a setting:
  - `type`: the table above, none of the rules after it
  - `type+risk`: the table with the rules after it, which is what `panel` does
  - `kind+risk`: all four for a question, a `design` flag or a risky path, the project manager and the advisor for every other ruling
  - `kind+risk-researcher`: the same with the researcher in the advisor's place
  - `pm+advisor` and `pm+researcher`: that pair on every ruling, with nobody above it
  - `all`: all four on every ruling

  For each: `decided` (rulings it would have decided without you), `matched` (of those, the ones it decided as you answered), `maintainer` (rulings it would have sent to you) and `cost`, per ruling, summed from the ledger lines of the votes it would have asked, an escalation's included. A call with no reported cost adds nothing, and `cost` is `null` when the usage ledger cannot be read. Only rulings answered under a build with `whatif` count, since an older line does not say whether it was a shadow run or a risky path.
- After a project's first 30 panel rulings (`facts_due` in `status`) `docs/facts/the-panel.md` gets an entry: cost per ruling and per expert, each expert's agreement and lone-correct count, abstentions, what each smaller panel would have decided against your answers, and the `whatif` table. Use it to keep, drop or downgrade an expert, move a type to a smaller or larger panel, pick the policy `panel` should use, or list more kinds under `rulings.pm_alone`.

## Upgrading

```sh
cargo install --git https://github.com/shep-pm/shep-kelpie --locked
shep kelpie upgrade                     # restart the dog and the runners onto the installed build
shep kelpie upgrade --ref main          # build shep-kelpie at a git ref and install it
shep kelpie upgrade --release 0.3.0     # install a release (none is published yet)
shep kelpie upgrade --binary ./kelpie   # install a build made by hand, as it is
shep kelpie upgrade --rollback          # put back the build the last upgrade replaced
shep kelpie upgrade --ref main --now    # restart the runners without draining them first
```

The installed shep-kelpie is whatever program the adopted dog runs, which the shepherd knows (`~/.cargo/bin/shep-kelpie` for the install above), and `shep kelpie add` registers runners that run the same path. An upgrade writes the new build beside that file and renames it over, as shep upgrades itself, so the running file's bytes are never edited in place. Before the swap it copies the file it replaces into `$SHEP_HOME/kelpie/builds` (resolved first if the path is a symlink), and `--rollback` puts that copy back the same way. Then it restarts the dog and each running runner one at a time, never while a work item is merging: it waits, and says what it waits on. A runner that does not answer `status` is waited on too, and named if it never does. A runner you stop while the upgrade waits stays stopped, and so does one stopped before it. A stopped runner starts on the new build when you start it.

Before it restarts a runner, the upgrade drains it, so the restart cuts no worker's turn short. The runner's `drain` trigger tells it to start no new call (a worker's turn, a reviewer's session or local round, the project manager's wake), while the calls it has running go on to their end and its merges go on as ever. The upgrade asks again until the runner shows no call running, saying what it waits on, then waits out any merge and restarts it. It waits no longer than the runner's turn ceiling (or the project manager's, if longer) plus five minutes, counted from when the wait began. A turn's ceiling counts from when it got the lease it waited for, and a reviewer's session has no ceiling, so either can outrun that bound. Past it the upgrade sends the runner `undrain`, so it starts calls again on the old build, names it, and restarts nothing for it, as for a merge that outlasts its wait. A runner on a build from before `drain` is restarted as before, and the upgrade says the restart cuts its calls short. `--now`, first or last, restarts without draining, for when you want the build in place at once; the merge wait still holds. The dog is never drained, since the calls are the runners'. A restart ends draining. So does a Ctrl-C or SIGTERM to the upgrade, or any failure, while a runner drains: the upgrade sends it `undrain` before it exits. Only a shepherd it cannot reach leaves one drained. The upgrade prints `shep kelpie undrain -p <project>` as each drain starts, for that case, and `shep kelpie status` marks a draining runner `(draining)`.

A bare `upgrade` is for after `cargo install`, which has already put the new build at the installed path. It runs the same checks as `--binary <installed path>` (the build's shep line against your shepherd's) and then restarts, with no staging and no swap. Kelpie records the installed file's size and modification time in `$SHEP_HOME/kelpie/builds/installed.seen` after every upgrade that finishes its restarts, and a bare run compares against it. If the file is the same, it says nothing changed since the last upgrade and restarts nothing. With no record, it counts as changed. If the file differs, it restarts and says that `--rollback` has no copy of the build cargo replaced (the previous build kept is the one before it), while `--ref`, `--release` and `--binary` keep one. `--now` and the drain apply as for the other forms.

If an upgrade stops after the swap (a merge or a call that outlasts the wait, a sheep that does not come back, Ctrl-C), the new build is already installed and the message says so. `shep kelpie upgrade --binary <installed path>` finishes the restarts and touches no file. `--rollback` does not: it swaps the two builds again. After a `--rollback` that stops, the older build is the one installed, and the same command finishes its restarts.

The upgrade reads the flock again just before the restarts, since the wait for a build or a merge can be long, but a sheep can still change after that read. A stop that lands between the re-read and the restart is undone: the restart starts that sheep again. A sheep started during the wait stays on the old build. In both cases run `shep kelpie upgrade --binary <installed path>` again, which restarts every running sheep and touches no file.

This build writes each project's state file as version 31, and every older build refuses a version 31 file, whatever it holds, so going back by `--rollback` or any other way leaves the older build unable to read it. An older build also never knew a work item that holds no slot: parked, waiting for one, or going on without one. Finish or drop those before going back. Draining only stops new calls and leaves them in the state file.

Every sheep it restarts must run the dog's path. If one does not, the upgrade stops before it changes anything and names the sheep and its path. A `--binary` without its executable bits is refused with the `chmod +x` that fixes it, and only one upgrade runs at a time: a second refuses while the first holds `$SHEP_HOME/kelpie/upgrade.lock`. The swap changes the file for every shepherd that adopts the same path, with no check against their shep.

Before it changes anything it asks the new build which shep it is made with (`shep-kelpie version --json`) and compares it with your shepherd's. If your shepherd is on another minor, or on an earlier patch than the build's, it stops before restarting anything and prints the steps: upgrade shep and reload its shepherd, then run the new build's own `upgrade` with `SHEP_HOME` set to your shepherd, since the old kelpie that `shep kelpie` runs refuses the new shepherd. The upgrade prints the exact line. `--rollback` checks the previous build the same way.

## What shep does for it

shep-kelpie is a dog of your own shepherd, and leans on it for everything a process manager does:

- **Supervision.** The adopted `kelpie` is the dog, and each project's runner is a sheep. shep starts, restarts and stops them, and keeps them beside your other sheep. `shep stop <project>` stops a runner, and `shep delete <project>` removes it
- **Lambs.** Every agent call a runner starts, a worker's turn, a reviewer's session or the project manager's wake, is a lamb of that runner. `shep describe <project>` labels each with its issue and role, such as `#114 worker`
- **Stopping the calls.** A runner asks for `shutdown_with_message`, so a stop reaches it as a message, and it sends each call's process group SIGTERM and exits at once, without waiting for them. shep's stop then ends every lamb the runner left, whatever process group or session it is in (shep-pm/shep#688, ADR 0005), so a runner's entry takes shep's default `kill_timeout`. A runner still ends a single call itself, at its turn ceiling, through the process group each call leads
- **Triggers and status.** Every `shep kelpie` verb is a trigger on the shepherd channel, which `shep trigger <project> <action>` sends too, and the runner answers while its calls run. Runners ask the dog for leases with metrics on shep's bus
- **Settings.** A project's settings are its runner's `[app.dogs.kelpie]` table, and shep-kelpie's own are the `[kelpie]` section of `dogs.toml`. shep keeps both, and lookout edits both from shep-kelpie's settings schema
- **Logs.** `shep bleats <project>` is a runner's log, and `shep bleats kelpie` the dog's

One thing shep-kelpie still does itself, until shep can:

- **Reaching you.** Rulings go to kelpie's own webhook, ntfy or Discord, and ntfy replies are checked with kelpie's own TOTP, until shep can carry a question itself (shep-pm/shep#689)

shep-kelpie refuses a shepherd on another shep minor or major than the pinned one, or on an earlier patch, naming both versions. It needs 0.12.4 or later, the first shep whose stop ends every lamb. shep-kelpie's commands talk to the shepherd's socket with the shep client it is built with, never a `shep` on `PATH`. Keep a shepherd's `SHEP_HOME` short: see [shep-kelpie's home](#shep-kelpies-home).

## Reference

### Commands

shep-kelpie runs in your own shepherd, beside your other sheep. The adopted dog holds the leases every runner asks before a summon, and it asks shep for the channel the lease commands reach it on. `add` and `start` say how to bring the dog up when it is not running with its channel.

```sh
shep kelpie add        # labels, settings, agent files, and the runner, stopped
shep kelpie start      # starts the runner, which runs the project
shep kelpie pause      # stops it once its calls end
shep kelpie finish     # stops it once its open work items end, picking nothing new
shep kelpie status     # every project, or --json for the runner's whole object
shep kelpie doctor     # what each project still needs on this machine
shep kelpie rule 14 yes
shep kelpie issue "<request>"   # issues for you to read, or --interactive
shep kelpie attach 7   # steer issue 7's worker in this terminal
shep kelpie watch      # follow the runner and its sessions live, or `watch 7`, `watch pm`
shep kelpie upgrade --ref main
shep kelpie tools install
shep kelpie github setup   # once per repo owner, with --org <org> for an organization
shep kelpie totp       # once, to answer rulings from ntfy
shep kelpie lease status
```

`status` prints a short block per project: its run state, each work item with its phase, turn and pull request, the rulings waiting, and the pacer.

```text
$ shep kelpie status
koji (finishing)
  run: finishing
  work items:
    #7 Add a thing: phase ruling, turn ended, PR #71
    #9 Fix the other: phase implement, turn running, no PR
  rulings waiting: 1
    ruling 1: merge, #7, PR #71
  pacer: on
    claude: session 3%, week 15%, 15% of the week spent today of 14.3% allowed
    claude holding (allowance): today's allowance is spent
```

`status --json`, before or after `-p`, prints each runner's whole object on one line, which the rest of this README names its fields from (`work_items`, `pacer`, `forge_held_until` and the others). Read it with `shep kelpie status -p koji --json | jq .pacer`, or `shep trigger koji status`. Without `-p`, a project that is not running, or still starting, is one line either way.

Every trigger a person sends the runner is also a verb: `add <issue>`, `rework <pr>`, `adopt <pr>`, `gate [<issue>]`, `drop [<issue>]`, `timings [<n>]`, `board`, `board move <issue> <position>`, `board top <issue>`, `tell "<note>"`, `pm`, `rule`, `drain`, `undrain` and `finish`. Each reaches the project whose repo holds the folder you run it in, a worktree of it included, or the one `-p <project>` names anywhere in the line. Run from your own clone of a project's repo, it finds the project whose `git.remote` is that clone's `origin`; two projects sharing one `origin` are named and `-p` is asked for. From anywhere else it lists the projects and guesses nothing. `shep trigger` still works. `add <issue>`, `gate`, `drop`, `timings`, `board`, `tell`, `drain`, `undrain` and `finish` print plain text, and the runner's JSON with `--json`, as `status` does; a refused answer prints its reason and fails either way. Where the runner gave no object to print, a `finish` on a stopped runner or a `tell` that timed out, `--json` prints `{"sheep": ..., "message": ...}` with the sentence the plain form prints.

- `drain` holds back every call the runner would start next, and answers the status with the calls still running under `draining`. `undrain`, or a restart, lets calls start again. `shep kelpie upgrade` drains each runner itself: see [Upgrading](#upgrading)
- `drop [<issue>]` ends a work item without merging it. Its worktree, local branch and build folder go, and its pull request stays, labelled `ready-for-human`. The board leaves the dropped issue be while it is labelled `ready-for-agent`, and says in its briefing and in `status` that it was dropped. Once the runner has read the issue without the label (at most once a minute, even while the board is shut), putting the label back returns it to the board; `shep kelpie add <issue>` takes it back at once
- `gate [<issue>]` sends a work item whose worker's turn ended with a pull request into the review gate, when it was never entered
- `tell "<note>"` and `pm` reach the project manager: see [The project manager](#the-project-manager)
- `timings [<n>]` totals where the time went over the last `n` finished work items (10 when left out), in six phases: `worker`, `review`, `ci`, `ruling`, `merge` and `other` (every second lands in exactly one), and prints the table, so `shep kelpie timings 20` shows it. With `--json` it prints the runner's object, the totals under `seconds` and the same text under `table`. `status` shows each open item's split under `timings`, and the ten most recent finished items under `history`. The state file keeps the last 100

`usage` reads the project's usage ledger, `<kelpie home>/<project>/usage.jsonl`, here, without the shepherd. Each model call adds a line to it as it ends: the runner writes the worker's turns, every reviewer's sessions and local rounds, the project manager's wakes, compactions and batch retros, and `shep kelpie issue` the issue writer's headless runs (not `issue --interactive`, which is your own `claude` session). Each line has the call's four token counts, its units, its dollars where the harness reports them, and the pacer's last reading where there is one (the issue writer's and imported lines have none). A finished work item adds a line of its own. `shep kelpie usage [<project>] [--since <date>]` prints, for each merged pull request, its units, dollars and the calls those dollars leave out because their harness reported no cost, wall time, rulings and the worker's and reviewers' shares of the units, with their medians, the loaded units per merged pull request (every call, the project manager's, the issue writer's and those of work items dropped or closed with no change included, over the merged count), the work items that ended with their issue closed with no change and those dropped, then the project manager's and the issue writer's totals, and the retros' when any ran. Every dollar figure says how many unpriced calls it leaves out, as in `$4.20 + 3 unpriced calls`; with no project it prints every project that has a ledger. Units weigh tokens as the control room was measured: cache read 0.1, one-hour cache write 2, five-minute cache write 1.25, output 5, uncached input 1. `shep kelpie usage <project> --import <steps file>` adds the turns a runner logged but the ledger lacks, its `ended` and `asked` lines in `<kelpie home>/<project>/steps.jsonl`, skipping any the ledger already holds. Baselines to compare with, such as the control room's own units per merged pull request, can go in `<kelpie home>/baselines/baselines.json`, which stays on your machine; `usage` holds each against kelpie's loaded figure and its median, as kelpie over the baseline, so below 1 is cheaper.

`usage --forge [--since <date>]` reads the forge ledger, `<kelpie home>/forge.jsonl`, here, without the shepherd. Every project's runner appends to the one file: a line for each call through the forge port (the maintainer's login, or the GitHub App's for the reads it makes) and each write or token of the GitHub App's voice, with its time, project, kind (`pull_request`, `reviewed`, `review_bot`, `app_comment` and so on), `rest` or `graphql`, `by` (`login` or `app`, whose rate budget the call spent; a line from before this field reads as `login`), milliseconds, and how it ended: `ok`, `failed`, `held` (never made, because an earlier answer had the limit used up) or `used-up` (GitHub said a rate limit is used up). A line is one port call, and a port call can ask GitHub more than once, so the counts are a lower bound on requests. A call that asks both APIs counts as GraphQL. The report prints the calls made, held and used up, then calls per project, per identity and per kind (calls, minutes with a call, the average over those minutes, the busiest minute), the ten busiest minutes with every project and kind in them, and each used-up answer, with `app` after its API where it was the App's budget, and the calls made in the 60 seconds before it (read from the whole ledger, so `--since` does not cut the window). The read of when a used-up limit resets (`rate_limit_reset`) is a call too, and is recorded. A minute is a clock minute in UTC, and held calls are left out of every count. Each line is one write to the end of the file, so runners appending together do not mix lines. A ledger that cannot be written is told once on stderr and costs no call.

`watch [<issue> | pm] [--back <n>] [--full]` follows a project live, in one stream, without asking the runner. Each line is a time, who, and one of the prompt kelpie sent, what the agent said or thought, a tool it called, a tool's result, or a runner step. Who is `runner`, `pm`, or `#<issue> <role> (<agent>)` for a worker, a reviewer or the issue writer. It starts at the end of the stream, or `--back <n>` lines before it, sorted by time. `watch 7` shows only issue 7's sessions and steps, and `watch pm` only the project manager's. A line is one clipped row; `--full` shows prompts, results and thinking whole. It reads the runner's steps, `<kelpie home>/<project>/steps.jsonl`, and the transcripts of the sessions they name, so it works for Claude Code, Codex and pi sessions alike, and from any terminal. A session is attributed by the id in the runner's `session-started` step, logged as its call starts, so a session started before this step existed shows its runner steps only. A project written by a runner from before `steps.jsonl` has none of its steps there, and `watch` shows nothing for them.

`add` names the project after the repo, or `shep kelpie add <name>`. It makes the four labels where the repo lacks them, writes kelpie's own agent files where they are missing, and registers the runner, holding the project's settings as its `[app.dogs.kelpie]` table. Running `add` again changes nothing. `start`, `pause` and `finish` find the project from the checkout, or take its name.

`doctor` changes nothing and prints one line per check, each missing piece with its fix, then exits non-zero if a project needs something it lacks. It checks that `claude` is installed and logged in, that `gh` is logged in and may push to each project's repo, the sandbox runtime every agent runs in, the shepherd's version, each project's checkout, implementers and labels, that kelpie's GitHub App is installed on each project's repo and mints a token, that a worker there holds a token that pushes and nothing more rather than your own `gh` login, which identity reads each project's board, pull requests, reviews and checks (the App, or your `gh` login, with the permissions an older App lacks), and that a ruling on a repo it covers has a login to mention (`git.maintainer`, else the one `gh` acts as) there, and, on macOS, the keychain-free copy of the sandbox runtime and the login token a covered project's `claude` needs, CodeRabbit where a project lists it, each project's reviewers, in order, with any command or endpoint among them checked as a runner's start checks it, Codex's usage for a project that spends it, each project's e2e host, where it names one, as `ssh` and `rsync` being on the `PATH`, and which webhook, if any, rulings post to. `shep kelpie doctor <project>` checks one project. `--test-alert` posts one test alert to the webhook, which is the only post it ever makes. A line marked `unsure` could not be settled, and does not fail the run: CodeRabbit is one, since a repo it has not yet reviewed looks the same as a repo without it. The GitHub App's line is `unsure` when GitHub is out of reach, busy or rate limited, and when the owner has no App, since kelpie then posts as it always did.

### Leases

The dog holds `cargo-test`, a share of this machine for running tests. There is no `gpu` lease: paddock queues the GPU, and the dog stays out of it. Workers run their test suites under `cargo-test`, and so can anything else on the machine: `shep kelpie lease run cargo-test -- cargo test` waits for a turn, runs the command and exits with its code, or 75 when it cannot reach the dog. Three hold it at once unless `[kelpie.leases]` says otherwise, the rest queue, and `shep kelpie lease status` shows who holds each lease and who waits. A command that ends or dies gives its turn back. One whose `lease run` was killed outright keeps it until the command, and anything it left running, exits: `status` shows how long each has held it.

`shep kelpie lease take gpu` and `shep kelpie lease run gpu` are refused, pointing at `shep paddock run --model <model> -- <command>`, which waits for paddock's lease on the model, holds it while the command runs and gives it back as the command ends. `sequences = 1` makes a lease an exclusive turn, so it is also how you take the GPU to yourself. `$PADDOCK_KEY` and `$PADDOCK_URL` reach it from your shell. No `gpu.lock` file is created by any path.

`shep kelpie status` shows a lease on a model behind a gateway in `local_leases`, under the model's name (`model@gateway` when a lock or another gateway's model has that name), with the lease's id, its client, its note as `what`, and when it was taken. That includes a lease you hold with `shep paddock run`.

`shep kelpie lease e2e` asks the dog to run the project's end-to-end tests (see [End-to-end tests](#end-to-end-tests)), prints the command's output as it arrives and exits with the command's own code. It exits 78 when the dog says the project has no e2e host, and 75 when it cannot reach the dog or the dog goes away mid-run.

### End-to-end tests

A worker's sandbox reaches GitHub and the project's `allowed_domains` and binds nothing local, so it cannot run tests that need a browser, a database or a service. A project can name a host that runs them instead, in an `[app.dogs.kelpie.e2e]` table:

```toml
[app.dogs.kelpie.e2e]
host = "e2e-box"
# command = "scripts/e2e-remote.sh"
# results = "test-results"
```

- `host` is an ssh alias from your `~/.ssh/config`, so the key, user and address stay out of kelpie's settings. It takes letters, digits, `.`, `_` and `-`, and may not start with `-`
- `command` is a file in the repo, `scripts/e2e-remote.sh` by default, and `results` is a folder in it, `test-results` by default. Both are relative, take letters, digits, `.`, `_`, `-` and `/`, and may not contain `..`, and `results` may not be `.git`
- A runner writes the table to `e2e.json` in the project's folder when it starts, and removes the file when the table is gone. A worker cannot write that file, and a change to the table waits for the runner's next start

Read this before you set one. The project's script runs on the host as the alias's account, and a worker edits that script, so a worker can run anything on the host that account can. Name a host that exists for this and holds nothing else: a disposable machine or VM, never your own, and no account that reaches your keys, your other hosts or your network. The ask a worker sends names none of host, command or results folder, so the dog takes them from the table alone. A worker runs `shep kelpie lease e2e` from its worktree (its instructions say so when the table is set), and the dog places the ask by which project's `worktrees` folder holds that folder. A project without the table, or a folder outside every worktree, is told it has no e2e host. The dog looks under `$SHEP_HOME/kelpie` only, so a runner started with another `KELPIE_HOME` cannot use e2e, and says so in its log.

What a run does, once the dog grants the host. The dog keeps one lease per host, so a second ask for the same host waits in the queue (`Queued`, as `cargo-test` does) and two hosts do not wait on each other. `shep kelpie lease status` lists each host's lease as `e2e:<host>`.

1. It copies the worktree to `kelpie-e2e/<project>` in the account's home on the host with `rsync -a --delete -e ssh`, leaving out `.git` and whatever the worktree's `.gitignore` ignores. A file the copy removes there is gone, so nothing carries over from an earlier run
2. It runs `./<command>` in that folder through `ssh -tt <host>`, and prints each line of its output as it arrives. The command's exit code is the run's. The terminal merges the command's stderr into its stdout, and nothing is sent to its stdin
3. It copies the results folder back into the same folder of the worktree, even when the command failed, with `rsync -rt --no-links --no-perms --no-owner --no-group`. Links in the results folder on the host are skipped, and nothing outside the folder comes back. The files land in a folder of the dog's own first, then go into the worktree one folder at a time without following a link. A link at the results folder, above it or inside it is refused, and the output says so, so a worker that swaps one in mid-copy sends nothing outside the worktree. A file already there is replaced, not written into, so a hard link or a FIFO the worker left in the results folder changes nothing else. The command's own code stays the run's

The dog starts `ssh` and `rsync` with argument lists and no shell, and no other program. It needs an `rsync` that takes those flags (3.x) on its `PATH`, and the macOS one may not. A worker that hangs up ends the run: `ssh` is killed, which hangs up the command on the host, and the next ask in the queue is granted. A run still going after 30 minutes is ended the same way, with a line saying so and the exit code 124. A command that outlives its hangup because it detached from the terminal keeps running on the host, so a script should run what it starts in the foreground.

An example host script, `scripts/e2e-remote.sh`, for a host that runs rootless podman and an app that builds from `e2e/Containerfile`:

```sh
#!/bin/sh
set -eu
mkdir -p test-results
podman build -q -t e2e-app -f e2e/Containerfile .
exec podman run --rm --userns=keep-id \
  -v "$PWD/test-results:/app/test-results" \
  e2e-app npm run test:e2e
```

Add `:Z` to the volume on an SELinux host. The script runs as the account's own user with no root, so what it starts is limited to what that user's podman can do.

`doctor` checks that `ssh` and `rsync` are on the `PATH` and names the host. It makes no connection to it.

### Settings

A project's settings are its `[app.dogs.kelpie]` table. `shep kelpie get [<key>]` prints it, or one dotted key, as TOML, and `shep kelpie set <key> <value>` changes one key, such as `shep kelpie set git.merging auto` or `shep kelpie set agents.reviewers '["qwen"]'`. The value is a TOML value, and text that is not one is a string. `set` writes only a table the runner would accept, and refuses to move `git.checkout` or `git.remote` onto a checkout or repo another project runs, as `add` does. It says when a running runner takes the change: at its next wake, or for `git.checkout`, `git.remote` and `hooks`, when it next starts. Lookout edits the table too, in the runner's pane. Every setting sits in a table of its own. `settings.example.toml` lists every key, and shows a runner's Flockfile entry for a project set up by hand. It needs `SHEP_HOME` as an absolute path in `env`, since a sheep starts without it. An older entry's `kill_timeout = "10s"` still works, and is no longer needed. The keys:

- `git.checkout`: the project's checkout
- `git.remote`: its GitHub repo as `owner/name`. Left out, the runner reads it from the checkout's `origin` when it starts, and an `origin` that is not a GitHub repo stops the runner naming the setting
- `git.merging`: who merges a green, reviewed pull request, `ask` or `auto` (see [CI and the merge](#ci-and-the-merge))
- `git.issues`: what becomes of deferred findings, `ask`, `file` or `skip` (see [The review](#the-review)). `ask` when left out
- `ci.block`: whether the repo runs CI that a merge waits for, and `ci.fix_attempts`: how many fix turns red runs get, `-1` for no cap (see [CI and the merge](#ci-and-the-merge)). `ci.test_command`: how to run one test file, holding `{test}` (its repo path) or `{name}` (its name without the extension), such as `cargo test --test {name}`. Left out, a reviewer is not asked for tests (see [The review](#the-review))
- `concurrency.active_items` and `concurrency.pending_rulings`: how many work items hold a slot, and how many parked on rulings stop the board opening new ones, which is not a cap on rulings (see [The board](#the-board))
- `rulings.decide`, `rulings.pm_alone` and `rulings.full_panel_paths`: see [Deciding](#deciding) and [Who votes](#who-votes)
- `agents.implementers`, `agents.reviewers`, `agents.pm` and `agents.fallback_after`: see [Agents](#agents). `agents.advisor`, `agents.researcher`, `researcher.transcripts` and `researcher.allowed_domains`: see [The panel](#the-panel). `agents.issue_writer`: see [Writing issues](#writing-issues)
- `pacing.enabled` and `pacing.kickoff_hours`: whether usage holds work, and the hours a day the per-hour figure in `status` divides the day's allowance by
- `worker.allowed_domains`, `worker.build_env` (variables that point tool caches into the build folder), `worker.instructions_file`, `worker.guard_hooks` (Claude Code hooks of the project's own, after kelpie's guard), `worker.turn_timeout` and `worker.compact_window` (the tokens of context at which Claude Code compacts a worker's session in the middle of a turn, 200000 by default, where it would otherwise wait for the model's whole window; `0` leaves it out. After a compaction the worker is handed the turn's prompt again. Reviewers and the project manager are not bounded)
- `hooks` is a list of `{ point, command }`: your own checks, which kelpie runs itself for every harness and role. A point is a gate, which can refuse, or a notice, which only tells. The gates are `publish` (any text before it goes to the forge, kelpie's own included), `commit` (a worker's message and staged diff), `push` (the commits being pushed), `handover` (a worker's finished branch, before review), `command` (every shell command an agent runs, with its `command` line and `cwd`), `write` (every file an agent writes or edits, with its `path`, `tool` and `content` or `patch`), `after_command` (a shell command's `exit` and output tail, once it has run), `turn_start` (a prompt kelpie is about to send a worker, reviewer or the project manager) and `turn_end` (a worker's turn, with its `last_message`). The notices are `reset` (before kelpie compacts a session), `ruling` (a ruling raised, with its `kind`, `question` and `choices`), `ci` (CI settled on a `head`, with its `conclusion` and `failed` checks) and `done` (a work item ended, with its `history` entry). The command gets a JSON payload on stdin. `commit`, `push`, `command`, `write`, `after_command` and a worker's `publish` run inside kelpie's sandbox, in the worker's fence, so they read only what the worker can; the rest run in the runner, outside it. At a gate, exit 0 passes, 2 refuses with stderr as the reason, and anything else, a hook that runs past 30 seconds included, refuses as a hook that failed. At a notice, a non-zero exit or a hang is logged by hook name and kelpie goes on. A refused commit, push, command, write or `gh` post goes back to the agent as the tool's error, a refused `after_command` as feedback on the result, a refused pull request file, branch or `turn_end` back to the worker once, and a refused `turn_start` or post of kelpie's own parks on a ruling naming the hook. Each command must resolve when the runner starts, and a runner reads `hooks` only then, so a change waits for its next start. A point that is not one of these stops the runner at load, naming the points there are. Use `hooks` for a check that must hold on every harness (a git gate is a `command` hook that reads `command` from the payload and exits 2 to refuse), and `worker.guard_hooks` for a Claude Code hook on a tool call kelpie has no point for
- `e2e.host`, `e2e.command` and `e2e.results`: an ssh host that runs the project's end-to-end tests, and the script and results folder in the repo. A runner reads them when it starts. See [End-to-end tests](#end-to-end-tests)
- `skills.<step>`: see [Skills](#skills)
- `code_graph.enabled`: whether the worker's and the reviewers' calls get a code graph of the worktree, off by default. See [Code graph](#code-graph)
- `repo_setup.plugins`, `repo_setup.own`, `repo_setup.roles`, `repo_setup.parts`, `repo_setup.skip` and `repo_setup.extra`: which of the repo's plugins and own files reach which calls, which of their parts load, and the plugins to add. See [Repo plugins](#repo-plugins)

shep-kelpie's own settings, shared by every project, are the `[kelpie]` section of `dogs.toml`, which lookout edits in the dog's pane. Start from `kelpie-settings.example.toml`, and keep `dogs.toml` private: the webhook's URL is a credential.

- `[kelpie.webhook]` is where rulings are posted, and the only way one reaches you away from the terminal. With none, a ruling shows only in the log, `status` and `shep kelpie rule`
- `[kelpie.leases]` sets how many hold `cargo-test` at once
- `gpu_metrics_url` is the GPU's Prometheus metrics page, such as `nvidia_gpu_exporter`'s `/metrics`. `status` then shows the GPU's load, memory, power and temperature under `gpu`, read every 15 seconds
- `codex_home` is where shep-kelpie's own Codex login lives (see [Agents](#agents))
- `[kelpie.gateways.<name>]` is a model gateway such as paddock, with its `kind`, `url` and `key_env` (see [Gateways](#gateways)). A runner reads them when it starts
- A change reaches a running runner at its next wake, within a minute when idle. `git.checkout`, `git.remote`, `hooks` and kelpie's gateways wait for its next start, and the log says so. The dog reads `[kelpie.leases]`, and each review bot's window from its agent file, only when it starts, so after a change to either run `shep restart kelpie`

A key kelpie no longer reads, such as `review.reviewers`, `coderabbit` or `planning`, stops the runner, naming the key and what replaces it. So does each top-level key from before the tables: `repo`, `forge`, `merge_authority`, `ci`, `max_items`, `max_parked` and `skills.ci` name the key that took each over, and `private_names` says to check text for your own words with a hook in `hooks` instead, which kelpie runs for every harness and role.

### shep-kelpie's home

shep-kelpie keeps everything under `$SHEP_HOME/kelpie`, or the folder `KELPIE_HOME` names:

- `agents`, `builds`, `codex`, `rulings`, `tools`, `totp` and `upgrade`, shared by every project
- `retro-reports/unverified`, with each batch retro the project manager wrote, one file each, which shep-kelpie never reads again
- `github`, with a folder for each owner's App: its id, slug and client id in `app.json`, and its private key in `key.pem`, readable by you alone
- `dog`, with the dog's book and its door, `lease.sock`. The adopted dog gets `SHEP_HOME` and no `KELPIE_HOME` from shep, so it is always under `$SHEP_HOME/kelpie`, even with `KELPIE_HOME` set for your own commands
- `<project>`, with the project's `state.json`, `board.md`, `steps.jsonl`, `usage.jsonl`, worker files, `worktrees` and `builds`, and `pm`, the project manager's folder

So a project can't be named for one of shep-kelpie's own folders, in any case, since a macOS volume reads `Github` as `github`. Socket paths must stay under 104 bytes, so a runner with a long `SHEP_HOME` refuses to start and names the path that is too long. Keep a shepherd's `SHEP_HOME` short.

A runner writes two logs. The shepherd keeps what it prints, one plain line per event such as `#303 dispatched to claude-sonnet-5 (medium)`, in `$SHEP_HOME/logs/<project>-0-out.log`, for you to read. It never holds JSON. The runner appends each step as one JSON object per line, with the time it was logged, to the project's `steps.jsonl`, beside the usage ledger, `usage.jsonl`. `watch` and `usage --import` read that file and not the out log.

A runner or the dog refuses to start while `~/.kelpie` still holds its files from before shep-kelpie's home moved under `$SHEP_HOME`. The message names the folder and says what to do with it: move `projects/<project>`, which holds the project's state, to `$SHEP_HOME/kelpie/<project>`, and delete `wt/<project>`, `targets/<project>`, `shots/<project>` and `playwright/<project>`. Kelpie makes worktrees and builds again, so delete `wt` and `targets` once the project has no open work item: an open work item's state holds the old folder's path, and kelpie cannot move it. Kelpie's state for a project now goes in `$SHEP_HOME/kelpie/<project>/` by default, or under `KELPIE_HOME` when that is set. `shep kelpie doctor` reports a project whose folder is still there, and `shep kelpie add` warns when it registers one. A runner refused this way stays stopped, with the reason in `shep bleats <project>`, until you clear it and run `shep kelpie start <project>`. A runner added before this release restarts instead, until you run `shep kelpie add` again in its checkout. A runner added before this release has no stop code in its flock entry, so it still restarts under shep's backoff until its `stop_exit_codes` is set to `[78]`, which lookout's settings pane can do.

## Design

For how it works and why:

- `GLOSSARY.md`: the vocabulary
- `docs/adr/`: decisions that are hard to reverse
- `docs/design/`: every current decision, its reason and where it lives, split by area
- `docs/design-log.md`: the full record behind them, and the test plan
- `docs/facts/`: the measured facts, one file per topic
- `docs/specs/`: each test series and its results

## License

MIT or Apache-2.0, at your option.
