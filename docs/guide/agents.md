# Agents

An agent is a file: `$SHEP_HOME/kelpie/agents/<name>.md` (or the `agents` folder of the home `KELPIE_HOME` names), YAML frontmatter and then a Markdown body. The name is the file's name without `.md`. An implementer's body is added to kelpie's own instructions for that agent, and an empty body adds nothing. A reviewer's body is its prompt (see [The review](review.md#the-review)).

```markdown
---
role: implementer
harness: claude-code
model: claude-sonnet-5-5
effort: high
---

Extra instructions for this agent, added to kelpie's own.
```

`role` is what the agent is for: `implementer`, an agent that builds a work item, `reviewer`, one that reads a pull request, a review bot included, `issue-writer`, the one `shep kelpie issue` runs, whose body is its prompt and which runs on `claude-code` alone (see [Writing issues](issues.md#writing-issues)), or `pm`, the project manager, likewise (see [The project manager](board.md#the-project-manager)), or `advisor` and `researcher`, the two experts besides the worker and the project manager who vote on each ruling, whose body is their prompt too and which run on any harness (see [The panel](panel.md#the-panel)). A key only another role takes, such as a reviewer's `paths` or `second_look`, stops the runner. `harness` is Claude Code, `claude-code`, pi, `pi`, which runs a model on an OpenAI-compatible server such as Ollama, or Codex, `codex`, on a ChatGPT plan, or goose, `goose`, which runs a model on an OpenAI-compatible server as pi does, takes `url` or `gateway` and `context`, and runs an `implementer`, a `reviewer`, an `advisor` and a `researcher`. A goose reviewer, advisor and retro read the repo through the same read-only shell as Codex's (`kelpie guard --read-only`) and write no file, except that a reviewer of a project with `ci.test_command` writes its failing tests with goose's `write`, held to its tests folder alone (`edit` stays refused). `model` and `effort` are what the harness runs. `tools`, on a `claude-code` implementer or reviewer, is the list `claude --tools` takes, in place of what its role starts with: `Bash,Read,Edit,Write,Glob,Grep,Skill` for an implementer, and `Read,Glob,Grep` for a reviewer. Name it for a project whose workers need more, such as `WebFetch`; the settings file still denies the tools that reach past the worker's fence. `extends: <name>` builds a file on another agent: it starts from that agent's keys and body, the keys it sets replace the parent's (`role` and `harness` too), and a file with no body takes the parent's. A bare name is a file in the folder, which may extend another. `extends: kelpie:<name>` is one of kelpie's own agents, taken from the kelpie build that runs, never from a file. A lighter `defect-hunter` is a file holding `extends: kelpie:defect-hunter`, `effort: medium` and `second_look: false`. `body: append` puts the file's body after the parent's, with a blank line between, in place of replacing it, at each step of a chain. Without `body`, a body replaces the parent's, and `append` is the only value `body` takes. The merged file is checked as any file is, and a refusal names the child. A parent that is no file, a `kelpie:` name that is no shipped agent, a `body` that is not `append`, or a cycle stops the runner, naming the file. The variant keeps its own name for its `agent:` label, ledger lines and pacing, and an edit to the parent reaches it at the next wake. Any other key stops the runner, and so does a file that does not parse or misses a key its harness needs, naming the file and the key. A `.md` file whose name is no agent's, such as a `README.md`, is skipped and named in the log. A runner sees an edit to the folder the next time it wakes, as it sees a settings change.

Kelpie ships `sonnet-high`, Sonnet 5.5 at high, the default implementer, `opus-high`, Opus 5.5 at high, for work that is hard to undo, `defect-hunter-lite`, the default reviewer, Opus 5.5 at medium reading once, `defect-hunter`, the same prompt at high effort with a second look, `iq3-xxs-review`, the default first reviewer where kelpie holds a gateway named `paddock`, `standards` and `spec`, the two axes of the `code-review` skill, each a reviewer that reads once (see below), `qwen`, the reviewer that runs the qwen-review script, the review bots `coderabbit`, `cubic` and `codex`, `issue-writer`, Opus 5.5 at medium, `pm`, the project manager, Opus 5.5 at medium, `advisor`, Opus 5.5 at high, and `researcher`, Sonnet 5.5 at high. A review bot's file takes `review_wait`, the minutes after a summon before a bot that gave no review is passed over for the pass, 45 when unset (see [The review](review.md#the-review)). The shipped agents are baselines kelpie owns, so an upgrade reaches them. `shep kelpie add` writes each one that is missing as a stub, `extends: kelpie:<name>` and nothing else (`qwen` only where the script exists), and never writes over a file. Edit a stub to say what differs: `model: claude-opus-5-5` keeps the shipped body and changes the model, and `body: append` with a body of your own adds to the shipped one. A file you write with a body and no `body` key replaces the shipped body, and nothing says so when the shipped one later changes.

Kelpie also writes each shipped agent whole and read-only to `agents/.defaults/<name>.md`, to read and to diff against your file. Nothing loads it, and an edit there is lost: `add` and the dog's start write it again, so an upgrade's restart refreshes it. Override in `agents/<name>.md` instead.

A project lists the agents that build its work items and the ones that review its pull requests, from those files:

```toml
[app.dogs.kelpie.agents]
implementers = ["sonnet-high", "opus-high"]
reviewers = ["defect-hunter-lite"]
```

An issue labelled `agent:<name>` (the prefix in any case) runs on that agent, which needs no more than a file whose role is `implementer`: it need not be listed. `implementers` is the pool an unlabelled issue is picked from, and the fallback chain below. With more than one listed, the issue writer labels any other issue before the board opens it; with one, it runs on that one. `implementer_choice = "order"` skips that call: an unlabelled issue runs on the first listed, and no `agent:` label is written on it. An issue's own label wins either way, and a name whose file is missing or holds another role is refused. Any implementer may be first, a local one included, and a list may be all local. `sonnet-high` alone when absent. An implementer's file must say `role: implementer`, and a reviewer's `role: reviewer`. When the runner starts, and when the implementer files or `agents.implementers` change, it makes the `agent:` label of every implementer-role file the repo lacks, listed or not. It never removes or changes a label, since removing one strips it from every issue, closed ones included.

A work item keeps the agent it opened on, and each turn runs that agent's file as it is then, with one exception. With `agents.fallback_after` set, in minutes, a work item's first turn that has waited that long for its model is stopped and moves to the next implementer listed after its own that no other work item is waiting on or moving to. Its first turn has no output and no session yet, so nothing is lost, and its next turn starts fresh on the new agent. The log's `fell-back` line names both agents. A later turn never moves, nor does an item whose label ends in `!`, nor one whose agent `implementers` does not list: its label was already the choice. With no implementer after its own free, it goes on waiting.

A turn waits for its model while it asks for a lease on it, from a gateway, while its gateway answers busy, or while it has shown no output for `agents.fallback_after` (10 minutes with that off). `status` shows it on the work item as `waiting`, with `since` and `why` (`lease`, `busy`, `silent` or `unreachable`), and the board as "Waiting for its model since". Its first output clears it. A turn whose model host cannot be reached, a pi call that got a 502 or 503 from it or had its connection refused or reset, is not parked on a ruling. It keeps its session and is tried again after 30 seconds, then after twice as long each time up to 5 minutes, and `status` shows `unreachable` from the first failure. Past 30 minutes since that failure it parks on a `turn-failed` ruling that carries the last error. The 30 minutes is not a setting, and `agents.fallback_after` never moves an item for this wait. A restart begins a new wait, since the retry is held in memory only. That covers a turn behind a gateway's lease, and a harness whose output kelpie reads as it comes, such as pi. A Claude Code turn is not reliably marked: headless Claude Code retries a provider's overloaded or rate-limit answer inside the call and says so only when it ends, and kelpie reads its activity from the session's transcript, which can hold lines before any output. So a Claude Code turn may never be marked waiting, and its fallback is unreliable.

`usage` says how an agent's usage is read, and so which account paces it: `claude` (the default on Claude Code) reads `/usage`, `codex` reads Codex's own 5-hour and weekly windows, and `none` is a local model that is never paced. It must be the harness's own reader, so leave it out: Claude Code reads `claude`, pi and goose read `none` and Codex reads `codex`.

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

Kelpie runs pi with a home of its own, so your `~/.pi` is never read. pi runs no Claude Code hooks, so a worker on pi can't have `worker.guard_hooks`, and the runner refuses to start with them. Kelpie's own checks still run on every command and file write, and so do your `hooks`. pi's own `grep` and `find` tools run `rg` and `fd`, which it looks for in that home's `bin` folder first and downloads from GitHub when it finds neither there nor on your `PATH`, which the sandbox refuses. Install `ripgrep` and `fd` (Debian's `fd-find` counts, since pi takes `fd` under either name), and `shep kelpie doctor` names a project with an agent on pi that still lacks one.

A pi call's sandbox allows no host of the model's, since the sandbox opens every port of an allowed host and Ollama's admin calls (pull, delete, create) answer beside chat. Kelpie runs a forwarder outside the sandbox for each call, and the sandbox allows only that. It passes `POST` to the server's `/v1/chat/completions` and refuses every other path and method, naming what was asked. The worker never sees the model's address. The forwarder dials `http://`, so `url` is the server's `http://` address: an `https://` one is refused when settings load.

### Gateways

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
- `shep kelpie set` checks the same agents for the same shape, and reads no key: your shell does not hold the runner's, and needs none to change a setting.
- On a `paddock` gateway, each call holds a lease on its model from its start to its end: a worker's turn, a reviewer's session, and an endpoint reviewer's round. The gateway evicts nothing it runs on midway. The ceiling counts from the grant, and a gateway too busy to grant it is asked again every 30 seconds until then. A call that ends or fails releases it, and a runner that dies stops renewing it, so it runs out two minutes later. A worker's chat calls name the lease its turn holds (`X-Paddock-Lease`), and a reviewer's name none, since paddock otherwise gives a request the first free turn of any kelpie lease on the model, which can be another call's and loses the first-come order of its own turn. A lease id with anything but ASCII letters and digits fails the take, since it goes in a header. A gateway that answers that the model is serving another call (`reason_kind: turn`) is a wait for a turn, shown as `waiting` for a worker and as a queued round for a local reviewer.
- Every chat call kelpie sends a gateway asks for batch priority (`X-Paddock-Priority: batch`), whatever its role, as does an endpoint reviewer's round. Paddock serves a request with no priority as interactive and serves interactive requests first, and kelpie runs unattended, so none of its calls sits ahead of your own. A worker's own `X-Paddock-*` headers never reach the gateway, and a model on a plain `url` gets none.
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

Each Codex call gets a home of its own with that login linked in, so no call sees another's sessions. A runner reads `codex_home` when it starts. A worker on Codex can't have `worker.guard_hooks`, the same as pi. Kelpie's checks run as Codex's own hooks: `confine` on every `apply_patch`, `guard` on every command. A reviewer, an advisor and a retro have no write tool of their own. Codex has no read tool, so they read the repo through a shell that runs `kelpie guard --read-only`: `cat`, `head`, `tail`, `ls`, `tree`, `grep`, `rg`, `find`, `wc`, `file`, `stat` and `nl` as plain words, with pipes between them. It refuses redirects, `$`, backticks, git and any other command. A call with no fence gets no shell.

A local implementer is one whose usage is `none`. Each account keeps its own daily allowance and 5-hour stop, shown under its name in `status.pacer`, and a new work item waits on every account its roles spend. `shep kelpie doctor` checks Codex answers for a project that spends it. A `none` agent is never paced, and kelpie queues nothing for it: a model behind a gateway waits in the gateway's queue, and `status.local_leases` shows the holder of the gateway's lease on it.

`status` shows each role's tokens in `by_role`, with `cost_usd` only for calls whose harness reports dollars. `unpriced_calls` counts the rest.
