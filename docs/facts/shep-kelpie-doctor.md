# `shep kelpie doctor`

- Every check reads through a port the runner already has, so what doctor finds missing is what a runner would stop on.
  - The ports: `Meter` for `claude` (`/usage` answers only for a logged-in account, and a `claude` that cannot start is a spawn error), `Forge::viewer` for `gh`, `Reviewer::check` for the local round, `Alerts` for the test alert, and the shepherd connection with its minor check.
  - Two `Forge` questions are new: `can_push` and `review_bot_seen` (shep-pm/shep-kelpie#112). `Host` is doctor's own port, for the sandbox programs, since Claude Code offers no probe for them.
  - `GithubApp`, which the runner takes up with #397, answers the App's line: its token for a project's repo says whether kelpie's App covers it (shep-pm/shep-kelpie#396).
- `gh repo view <repo> --json viewerPermission` answers `ADMIN` on this repo for the maintainer and `READ` on `cli/cli`. `ADMIN`, `MAINTAIN` and `WRITE` push. `gh search prs --repo <repo> --commenter app/coderabbitai --limit 1 --json number` answers `[{"number":649}]` on shep-pm/shep and `[]` on `cli/cli`. Recorded in `src/adapters/gh/access.rs`.
- GitHub gives no way to ask whether an app is installed on a repo without the app's own credentials, so CodeRabbit "installed" means it has commented on a pull request there. A repo it has not reviewed yet reads the same as one without it, so that line is `unsure` and never fails the run. A repo GitHub marks private that lists CodeRabbit is `MISSING`, which is what the runner refuses at start.
- The sandbox needs `sandbox-exec` on macOS (`/usr/bin/sandbox-exec` is there on the maintainer's machine) and, on Linux, `bwrap` and `socat`, from Claude Code's documentation. The Linux half is not measured.
- Measured 2026-09-29 with this branch's build on a scratch shepherd (shep 0.11.0) and a scratch kelpie home, on a clone of shep-pm/kelpie-scratch:
  - `add` then `doctor` printed `ok` for `claude`, `gh`, the sandbox, the shepherd, push access, the three labels, CodeRabbit and the local command, and `MISSING` for rulings, since that shepherd had no `webhook` table. It exited 1 and the flock was as `add` left it.
  - With no shepherd running it printed the missing shepherd and skipped the projects. `--test-alert` with no webhook posted nothing.
