# The GitHub App

Per-user App: tokens, posting, answering rulings on GitHub, ruling panel.

## App and tokens

- Rulings, notices, rounds move to GitHub via an App each user registers (GitHub never notifies of your own comments); ntfy stays optional. One App per owner via manifest flow (shared App would need key shipped or hosted). #395
- App holds contents, workflows, issues, pull requests write + checks, statuses, metadata read; merging stays on user's `gh`, App never merges. Kelpie's own token: issues + PRs only, can't push. #395, #396
- Covered worker call gets a token for one repo, `contents` (and `workflows`) only; refused if GitHub grants more (guard parsing can't be complete; credential is the boundary). #420, #462
- Token reaches worker as a `gh` config file, not a variable (every process shows those); git credential helpers emptied (keychain never asked). #420, #429
- Covered worker writes `pull-request.md`; kelpie opens and edits the PR (worker's token can't). Worker files read/written without following links (planted link could leak a secret). #420, #439, `src/worker_folder.rs`
- Workers can push any branch: `doctor` says protect `main`. Kelpie reads covered workers' failed CI logs (3 runs, 6000 bytes each, after cargo's progress lines and GitHub's timestamps are dropped) rather than widen token. #420
- Worker token refreshed every 30 min during a call (lasts 1h). `src/runner/login_refresh.rs`
- `shep kelpie github setup` registers an App from 127.0.0.1 with random `state`, refusing other hosts. #396
- App under `<kelpie home>/github/<owner>/`, key 0600, never in argument or variable (like TOTP secret). #396
- JWTs signed with `openssl`, sent by `curl` config on stdin (not in process list, no RSA crate). #396
- Tokens cached per repo until 5 min before expiry; one mint flight per repo (slow mint stalls no other repo). #397
- `doctor` checks each repo's token; no App = `unsure`. #396

## Reading

- Board, PR, review and check reads (`issue`, `ready_issues`, `open_pull_requests`, `pull_request`, `reviewed`, `review_bot`) go through the App's reading token where it covers the repo: the App's own rate budget takes them off the login's. Viewer, access checks, `sub_issues`, `comments_since`, failed run logs and every write stay on the login. `sub_issues` stays because the token reaches one repo and a parent's sub-issues can sit in another, so a list from the App could be short and `close_merged` would close the parent early. #582
- Same `gh` commands as the login's, run with `GH_CONFIG_DIR` on a folder of kelpie's own (`<kelpie home>/reads/<owner>--<repo>/gh`) holding the token, and `GH_TOKEN` and its kin unset (an environment token would win). Not curl/GraphQL: custom queries can't be checked live from a worker. `src/adapters/gh/app.rs`. #582
- Reading token is its own grant (`Grant::Reader`: issues, pull requests, checks, statuses read), kept apart from the posting token; an App without checks/statuses 422s and falls back to `ReaderCore`. The token's returned `permissions` say what it holds. `src/github/tokens.rs`. #582
- `RateHeld` routes (`src/ports/rate_limit/app_reads.rs`): a read the token lacks a permission for, or that answers "Resource not accessible by integration", goes to the login and says the permission, once per repo and read. App reads take no turn in the login's pace; the App's used-up answer sends its reads to the login until it resets and holds nothing else. #582
- Forge ledger `by` (`login`|`app`) records whose budget a call spent; `usage --forge` splits by identity. `doctor` has a `reads` line. #582

## Posting

- Kelpie's posts go through `Voice` port as the App on a covered repo, never falling back to maintainer's login (notifies no one). Failed post never fails its step; told once in log. #397
- Summons, PR labels, merges, ready marks stay on maintainer's `gh` (bots answer a person; nothing shows they answer an App). #397
- Ruling comment mentions `git.maintainer` (else `gh` login) and says how to answer. Own review rounds post on the PR (bots post theirs). #397, #376
- Ruling comments share one card format so reactions and ticks read same options. #464, `src/runner/voice/card.rs`
- Ruling settled elsewhere has its comment edited. #472
- Worker posts no comment (as maintainer it could answer a ruling). Guard reads `gh` as gh does, refuses shell expansion. #418, `src/guard/gh.rs`

## Answering on GitHub

- Answered by reply in thread, read once per `BOARD_POLL` without runner lock. #398
- Only the mentioned login with write access answers; merge answer names its id (no conversational yes merges; strangers can't make kelpie speak). #398
- Kelpie's own `gh` comments recorded when posted, never read as answers (same login). #398
- Reading starts from GitHub's timestamp on the ruling comment, never local clock. #398
- First answer from any channel wins; ruling watched 1h more to say it's answered. #398
- Thumbs up/down from mentioned login answers yes/no ruling; tick counts only if that login or the App made every edit (GitHub doesn't say who ticked). #464

## Ruling panel

- Once `agents.advisor` or `agents.researcher` set, ruling with 2+ options gets blind panel: worker, PM, advisor, researcher. #465
- A drain asks no expert: a ruling raised in one waits for the restart or `undrain` and takes its panel then. Still waiting after the hold, it goes to the maintainer with `skipped: draining` (also `posted`), kept in `rulings.jsonl` and counted in `status`. #711 A question put to the PM stays the PM's through a drain; the panel does not take it up.
- Comment waits up to 20 min for votes (`panel::HOLD`), each expert 15 min. Vote `{"choice", "why", "design"}`; malformed abstains. #465, `src/runner/flight/panel.rs`
- Advisor (Opus 5.5 high) reads codebase; researcher (Sonnet 5.5 high) reads past rulings, threads, transcripts, web limited to `researcher.allowed_domains` (page may steer it). #465, `src/transcripts.rs`
- `rulings.decide`: `shadow` (default), `panel` (3 agreeing votes, no `design` flag decide; splits and merges under `ask` go to maintainer) or `always` (see budget.md). Reopen within 24h with `<id> no <note>`. #466, `src/runner/panel/decided.rs`
- PR title type sets who votes, smallest panel first. Questions and diffs touching `rulings.full_panel_paths` get full panel (agent under scrutiny chose the type). #466, #536, `src/panel/tally.rs`, `src/panel/policy.rs`
- Policies scored from shadow votes as `whatif`: fixed list in code, not a setting (a setting can't say what a choice costs). #536
- `rulings.pm_alone`: PM decides listed kinds alone, never `merge` (one voice must not merge). #466, `src/settings/rulings.rs`
- Votes are ledger entries; `status` reports agreement with maintainer (trust by record). #466, `src/runner/panel/measure.rs`
