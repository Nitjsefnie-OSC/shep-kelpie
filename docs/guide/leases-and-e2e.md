# Leases and end-to-end tests

## Leases

The dog holds `cargo-test`, a share of this machine for running tests. There is no `gpu` lease: paddock queues the GPU, and the dog stays out of it. Workers run their test suites under `cargo-test`, and so can anything else on the machine: `shep kelpie lease run cargo-test -- cargo test` waits for a turn, runs the command and exits with its code, or 75 when it cannot reach the dog. Three hold it at once unless `[kelpie.leases]` says otherwise, the rest queue, and `shep kelpie lease status` shows who holds each lease and who waits. A command that ends or dies gives its turn back. One whose `lease run` was killed outright keeps it until the command, and anything it left running, exits: `status` shows how long each has held it.

`shep kelpie lease take gpu` and `shep kelpie lease run gpu` are refused, pointing at `shep paddock run --model <model> -- <command>`, which waits for paddock's lease on the model, holds it while the command runs and gives it back as the command ends. `sequences = 1` makes a lease an exclusive turn, so it is also how you take the GPU to yourself. `$PADDOCK_KEY` and `$PADDOCK_URL` reach it from your shell. No `gpu.lock` file is created by any path.

`shep kelpie status` shows a lease on a model behind a gateway in `local_leases`, under the model's name (`model@gateway` when a lock or another gateway's model has that name), with the lease's id, its client, its note as `what`, and when it was taken. That includes a lease you hold with `shep paddock run`.

`shep kelpie lease e2e` asks the dog to run the project's end-to-end tests (see [End-to-end tests](#end-to-end-tests)), prints the command's output as it arrives and exits with the command's own code. It exits 78 when the dog says the project has no e2e host, and 75 when it cannot reach the dog or the dog goes away mid-run.

## End-to-end tests

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
