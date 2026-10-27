# The panel

Closed at this build (ADR 0018): the panel is gone, and the project manager answers the kinds of ruling `rulings.pm_decides` gives it (see `docs/design/who-answers.md`). What follows is what was measured while the panel ran. None of it describes kelpie now.

- Checked by the stand-ins only (#465): four votes filled a merge ruling's panel, an unreadable vote was an abstention, no prompt or folder carried another expert's vote, the settings of each role denied every writer and command (and the advisor the web) and read only the folders named, and the researcher's folder held what was written for it.
  - No live Claude Code, Codex or pi run ever voted on a ruling, and no expert's model was tried on a real ruling, so whether Opus 5.5 at high or Sonnet 5.5 at high was right for the advisor and the researcher was never measured.
- Never checked before it closed:
  - That the sandbox's `srt` let Claude Code's `WebFetch` reach a host named in `researcher.allowed_domains` (the allowlist was `strictAllowlist`, and a bare `*` was not tried).
  - That Codex writes `cwd` in its `session_meta` line, which the researcher's table read from what kelpie knew of Codex's rollout format, since no recorded rollout in `fixtures/` held one.
  - That a Claude Code session resumed for the worker's vote saw the tools of a retro without its `Skill` call failing.
- Checked by the stand-ins only (#466):
  - 4-0 and 3-1 decided under `panel` and not under `shadow`; a 2-2 split, a `design` flag and an `ask` merge at 4-0 went to the maintainer.
  - A `docs` item's ruling called only the project manager and a `fix` item's the project manager and the advisor; a split or a `design` flag in a smaller panel asked the next one up.
  - A kind on `rulings.pm_alone` was decided by the project manager's vote with no other call, a `design` flag or an abstention called the type's panel, and a merge kind stopped the runner naming `rulings.pm_alone`.
  - A `no <note>` reply reopened a decided ruling.
  - Every vote was a ledger line with its ruling, kind, expert, panel, cost and wall time; the usage report split them by expert, kind and type; and the streak, the per-kind run, agreement and lone-correct count followed the maintainer's answers.
  - Also (#536): under `panel` a `fix` item's question asked all four, a `docs` item whose diff touched a `full_panel_paths` glob asked all four, a `design` flag from a smaller panel asked the full one, and an empty list changed nothing.
  - `whatif` matched hand-computed counts and costs over recorded shadow rulings and ledger lines for all seven candidates, an escalation's votes included.
  - No live run decided a ruling, and the work item's type was read from the stand-in forge's titles, not from a real pull request.
- The Facts entry due after a project's first 30 panel rulings (cost per ruling and per expert, agreement, lone-correct counts, abstentions, what each smaller panel would have decided, and the `whatif` table) was never written, so which experts and which policy were worth their cost was never measured.
- The local test run could not show the experts' file-tool reads: a worker's sandbox cannot list the maintainer's `~/.shep`, where its temporary folder is, so the panel's two fence tests failed there and passed with that one case let through. CI, whose temporary folder is `/tmp`, ran them as written.
