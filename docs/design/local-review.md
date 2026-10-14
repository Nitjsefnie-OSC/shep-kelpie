# Local review

Local reviewers: `command` and `endpoint` agent files. The pass they sit in is in review.md.

## Reviewers

- Per project, checked at start; `endpoint` chunks the diff by `context`; `command` keeps the qwen script contract. #107
- Holds no lock (`lease` refused, no `/api/ps` check, no `local-model-spilled`); a running round is never preempted (kill wastes the work). #145, #502
- A round ends `answered`, `cut_off` (length, or inside its thinking), `unreadable`, `busy` (HTTP 429 or 503), `unreachable` (timeout, connection) or `failed`. `ReviewerError` carries it, `RoundEnd` names it.

## Kept rounds

- Each round writes under the work item's build folder, which goes with the item. #849 keeps them so the experiments repo can replay real rounds at other thinking limits.
- `[local_rounds]` in the project's settings: `keep = true`, `keep_days = 30` (0 keeps all), `folder` (absolute or `~/`; default `local-rounds/` in the project's kelpie folder). `shep kelpie set -p <project> local_rounds.keep false` works as for any setting; a relative `folder` or negative `keep_days` is refused with the reason.
- When a local round ends, however it ends, `src/runner/review/keep.rs` copies the round's own files, as written, to `<folder>/<issue>-r<round>-<UTC end time>/`: `request-N.json`, `reply-N.json`, `round-<k>.txt`, and a `command` reviewer's `criteria.md` and `hunks/`. A round the runner stopped is not kept: it did not end.
- `round.json` beside them: `project`, `issue`, `pull_request`, `round`, `reviewer`, `harness`, `model`, `max_tokens`, `base`, `head`, `started`, `ended`, `seconds`, `outcome`, `reason`, `findings` (sent to the worker). A round that read nothing (all findings unreviewed) is `failed`.
- Pruning removes only directories in `<folder>` named like a kept round, older than `keep_days`, whose `round.json` names this project, so projects may share a folder. It runs at runner start and after each kept round. Other entries are never touched.
- `keep = false` keeps nothing and prunes nothing: today's behaviour. A failed copy is a note, never a failed round.
