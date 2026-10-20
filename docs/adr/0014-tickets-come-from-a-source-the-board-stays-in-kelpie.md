# Tickets come from a source, and the board stays in kelpie

This settles the ticket half of #724. ADR 0002 stands: the runner still holds the board.

Kelpie reads its tickets from GitHub issues in the pull request's own repo. One `Forge` (`crates/core/src/ports.rs`) holds both the issues and the pull requests, and a work item, its branch, its folders, its rulings and the state file are all keyed by the issue's number. That rules out two kinds of project: a repo whose maintainer wants kelpie's pull requests but not its issues, labels and comments, and a project whose tickets live somewhere else, such as ClickUp (#270).

The decision:

- A ticket lives in exactly one source, and that source is its truth. Kelpie reads it and writes to it through a `Tickets` port: what is ready, read one, claim it, comment on it, close or move it, and its parent and blocked-by links. Kelpie keeps no copy of a ticket beyond what the board already remembers.
- The board stays in kelpie. The saved order, the project manager's holds and picks, drops and the file-overlap check are kelpie's scheduling, whichever source the tickets come from.
- Each project names one source. GitHub issues in the pull request's repo is the default and behaves as it does today. The second is a local store in kelpie's home, for a repo kelpie should write nothing to but its pull requests. ClickUp follows as a third (#270).
- A ticket's id is opaque to kelpie: whatever its source gives it. A GitHub ticket's id is its issue number, so a GitHub project's branches, folders and triggers keep their names. A local ticket's id carries a prefix no GitHub number has.
- A local ticket is a markdown file in `<kelpie home>/<project>/tickets/`: its title, labels, state, parent and blocked-by in front matter, and its text as the body. Its id is `k` and a counter, such as `k12`, so its branch is `kelpie/k12`. `shep kelpie ticket new`, `list`, `show`, `edit` and `close` file and read them.
- Pull requests, CI, review threads and merges stay on the `Forge`. Only a GitHub ticket's pull request ends with `Resolves #N`. For any other source, kelpie closes the ticket itself when the pull request merges.

## Considered options

- **Kelpie owns the tickets, and GitHub and ClickUp sync into them.** Every linked ticket would exist twice, so each source would need a sync with rules for which side owns which field, for conflicts, and for deleted or moved issues, and someone working in GitHub or ClickUp would see a copy that lags. It buys one view across sources and blockers between them, which nothing needs yet.
- **A `shep-kanban` dog holding the local store.** Set aside in #724, and still not now: the store sits behind the same port, so it can move into a dog once there is a GUI or a second consumer.
- **More than one source per project.** A project's tickets would need ids that name their source everywhere, and a blocker could point across sources. One source per project covers every project kelpie runs.

## Consequences

- The state file's work items, rulings, finished, dropped and saved order are keyed by ticket id. Its migration reads an older file's issue numbers as GitHub ids.
- Triage, adoption and contributions stay GitHub-only, since they are about issues and pull requests other people open there. A project on the local store has no triage.
- The issue writer and follow-ups file through the project's source, so the guard on filing learns a verb besides `gh issue create`.
