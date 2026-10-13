# Contributions

Whether and how kelpie picks up other people's pull requests. Today it works only its own; these settings come first, and nothing reads them yet. #617

## Settings

- `[app.dogs.kelpie.contributions]`, `#[serde(default)]`: an absent table is `pick = "off"`. `deny_unknown_fields`, so a mistyped key is refused naming it. `src/settings/contributions.rs`
- `pick`: `off` (default, kelpie's own pull requests only), `label` (those the maintainer labels for it) or `all`. Kebab-case, any other word refused.
- `reviewers`: ordered agent files whose role is `reviewer`, bots welcome, each once. Absent, it is the project's `agents.reviewers`, default included.
- `security_reviewer`: a reviewer agent file, `security-reviewer` when absent. #619 ships that file.
- `quiet_days` (7) and `nudge_days` (3): whole days above 0. `nudge_days` must be below `quiet_days`: the nudge comes before a contribution counts as quiet. A zero is refused by the parser, the ordering by `Contributions::check`, called from `Settings::from_table`, so a bad pair is refused by key when the settings are read and a reread keeps the old settings.

## Checks at start

- `Settings::contribution_lineup` shares its body with `lineup` (`src/settings/reviewers.rs`), so the bot-order check applies. Its refusals name `contributions.reviewers` when that key is set, and `agents.reviewers` when it falls back.
- Agent files are checked only when `pick` is not `off`, so a project that never opts in needs none.
- `Runner::open` refuses a `pick` other than `off` when `ports.voice.covers(&remote)` is false: `SettingsError::Invalid` on `contributions.pick`. The representative would otherwise post as the maintainer, which no worker or reviewer may do. `pick = "off"` starts on any repo. `src/runner.rs`
- `security_reviewer` is not checked at start yet: its file arrives with #619.

## Rereads

- `reread` keeps the running `contributions` and logs it as waiting for the next start, like `git.*`, whatever else changed: the checks above run only when the runner opens. `src/runner/reread.rs`
