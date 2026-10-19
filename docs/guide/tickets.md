# Local tickets

A project can take its tickets from files in kelpie's home in place of GitHub issues, for a repo that should get nothing from kelpie but its pull requests.

```toml
[app.dogs.kelpie.tickets]
source = "local"   # "github" when absent
```

A change waits for the runner's next start, and a work item already open keeps the ticket it was opened on, in whichever source holds it. A project with no `tickets` table behaves as it always did.

## Filing and reading

```sh
shep kelpie ticket new "Add a thing" --body "Do it." --label ready-for-agent
shep kelpie ticket list
shep kelpie ticket show k1
shep kelpie ticket edit k1 --blocked-by k2
shep kelpie ticket close k1
```

- `new` takes the title, `--body`, each `--label`, `--parent` and each `--blocked-by`, and prints the id: `k` and the next counter, such as `k1`.
- `list` prints the open tickets, `list --all` every one.
- `edit` changes the title, the body, the labels (`--label`, `--unlabel`), the parent (`--parent`, `--no-parent`) and the blockers (`--blocked-by`, `--unblock`).
- `close` takes `--not-planned` or `--duplicate` for the other reasons.
- Each verb works on the project you name, or the one the folder holds. They take local ids only, and do not check that the project's source is `local`.

A ticket is a markdown file, `<kelpie home>/<project>/tickets/k1.md`. Its title, labels, state, parent and blocked-by are in front matter, its text is the body, and its comments follow it. Editing the file by hand is fine; a file kelpie cannot read stops the board read and names the file, and `doctor` says so.

## How kelpie works one

- `ready-for-agent`, `priority:` and `agent:` labels, blocked-by and parents work as they do on GitHub. A ticket blocked by an open ticket waits until the blocker closes, and a blocker the store does not hold counts as open.
- The worker's branch is `kelpie/k1`, and its prompt carries the ticket's text rather than telling it to fetch one.
- Its pull request has no `Resolves` line. Kelpie closes the ticket when the pull request merges, and says nothing on GitHub.
- A ticket with no `agent:` label goes to the project's default implementer.
- `shep kelpie issue` files local tickets, `ticket new` in the store, and a finding a merged work item deferred becomes a local ticket labelled `needs-triage`, or a comment on an open ticket that already holds it. Label one `ready-for-agent` with `ticket edit` to put it on the board.
- `doctor` prints a `tickets` line: the source, and for `local` how many tickets the store holds.

Not here yet: keeping every other file out of the repo. Triage, adoption and contributions stay on GitHub's issues and pull requests.
