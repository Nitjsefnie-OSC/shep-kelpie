# Local tickets

A project on `tickets.source = "local"` takes its tickets from files in kelpie's home, not GitHub issues (ADR 0014). The default, `github`, is unchanged.

- `[tickets] source` (`src/settings/tickets.rs`). `Runner::open` puts a `Routed` port (`src/local_tickets/routed.rs`) over the held GitHub port and `LocalTickets` over `ProjectPaths::tickets()`, `<kelpie home>/<project>/tickets/`. `Guarded` still checks writes. A changed source waits for the next start. #1011
- A call about one ticket goes to the source its id names, so a work item opened before a change of source keeps working. The board's reads (`ready_issues`, `finished_parents`) go to the project's source. `create_issue` and `open_issues` stay on GitHub, since `open_issue` files a GitHub issue and needs its number, until follow-ups file to the project's source (#1012). Only GitHub calls are held and ledgered.
- A ticket is `k<n>.md`: YAML front matter (title, labels, state, parent, blocked-by), the text, then comments, each after a `<!-- comment: <author> <unix> -->` line. A text holding such a line is refused. `src/local_tickets.rs`, `src/local_tickets/file.rs`
- The next id is the highest on disk plus one, taken with an atomic hard link, so two `ticket new` calls never share one. A change to a ticket (`LocalTickets::update`) holds `k<n>.md.lock` while it reads and writes back, so the runner and `ticket edit` never lose each other's change. A lock older than 30 seconds is taken for a dead process's, and broken by one caller at a time, under `k<n>.md.lock.break`, which looks at its age again first. Scratch `.new-*` and `.md.tmp` files are ignored. A link to a ticket the store lacks is refused when added. A missing blocker counts as open.
- An unreadable file fails the board read, naming the file, rather than hiding a ticket.
- `ready-for-agent`, `priority:` and `agent:` labels, blocked-by and parents read as on GitHub. `LocalTickets::open_issues` is empty, since there is no triage. The issue writer's label step is skipped, so a local ticket goes to the default implementer or its `agent:` label.
- `shep kelpie ticket new|list|show|edit|close` runs on the files directly (`src/flock/ticket.rs`, `src/flock/verbs.rs`), for the project named or the one the folder holds. It does not check the source, and takes only local ids.
- The worker's prompt carries the ticket's text. Its instructions drop the `Resolves` line (`profile::instructions_for`, `src/runner/instructions.rs`), so the pull request has none.
- On merge, `close_merged_issue` closes a local ticket in the store (`close_local`, `src/runner/close_merged.rs`), with the same retries as GitHub's path. It posts no comment, and a local parent gets none when it closes.
- The review's criteria read a local ticket through `Tickets::issue` like any other.
- `doctor` prints a `tickets` line: `github`, or `local` with the count and folder, missing when the store is unreadable.
- Not here: filing from the issue writer and follow-ups (#1012), a project that writes nothing to its repo (#1013).
