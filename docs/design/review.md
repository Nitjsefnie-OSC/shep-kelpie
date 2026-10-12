# Review

Review passes, bots, CI, the merge, rulings.

## Review

- One pass when CI green on draft head: `agents.reviewers` in order, each once; no loop or judge (gains come in the first two reads). ADR 0006, ADR 0008, `src/runner/review/`
- Any finding, nits included, goes to one fix turn; next reviewer reads the fix (skipped nits left open threads). Nit-only fix starts nothing new; bot nits on a head it read stay open. ADR 0007, #354
- Round waits for CI on its head first (red costs no read or window); `doctor` names CI that skips drafts. #423
- Pass restarts only on a new head that needs it (merge `rework`, accepted foreign change, rework). ADR 0006
- Merge ruling `no <note>` = fix turn back to the ruling, no new pass; restarts pass only after unread/open-thread warning. #354
- Reviewer = agent file on session harness (read-only), `command`, `endpoint` or `bot`; runs no commands (running tests found fewer bugs). #314
- `paths` limit a reviewer to matching changes; every round reads `<base>...HEAD`. Gets issue title + acceptance criteria (4,000 bytes), so no whole-issue check. #314, #484
- `defect-hunter` (Opus 5.5 high, maintainer's prompt verbatim) uses `second_look`. `add` lists `["qwen", "defect-hunter"]` where qwen script exists, else `["defect-hunter"]`. `agents/defect-hunter.md`, `src/flock/verbs.rs`
- `standards` and `spec` (Opus 5.5 high, one read, no `second_look`) are the two axes of the vendored `code-review` skill, one agent each since parallel sub-agents are forbidden. Smells are `LOW`; `add` writes both and lists neither, and a project's `reviewers` list replaces the default, so it names them with `defect-hunter`. They sit beside `defect-hunter`, which finds behaviour defects they do not. `agents/standards.md`, `agents/spec.md`, `src/agents.rs`. #363
- Reviewer failing 3 times passed over; pass nobody read marks item unreviewed (API outage can't auto-merge). #314
- Round reads only pushed head; dirty worktree gets one turn to push, then parks `unpushed` (CI and merge take `origin`'s head). #290
- Merge trusts only heads a round read or kelpie sent to CI unread on purpose; others raise `unread_head`. #290
- Fix turn counts only if it moves `origin` head, unless it deferred all or got only nits; else parks `fix-not-pushed`. #40, #354
- Clean reply is exactly `CLEAN`. #314
- Local `command`/`endpoint` reviewers per project, checked at start; `endpoint` chunks diff by `context`; `command` keeps qwen script contract. #107
- Local reviewer `lease` names its lock; CPU-spilled model parks `local-model-spilled`; running round never preempted (kill wastes the work). #145
- One finding shape for every reviewer kind; bots are one provider with a tested profile each. #123
- With `ci.test_command`, reviewer's attached failing test runs before/after fix, recorded, blocks nothing until trusted. #517, #518
- Deferred finding kelpie sent is filed after merge in kelpie's words (nits dropped). `git.issues`: `ask` (default, `follow-up` ruling), `file` (as `needs-triage`; auto-filed were often false), `skip`. #142, #380, #384, `src/runner/follow_up.rs`

## Bots

- Bots = `harness: bot` reviewer files named for the bot (its account owns the window): CodeRabbit 1 in 1h, cubic 20 in 720h, Codex 10 in 168h. #315
- Bot round marks draft ready (CodeRabbit skips drafts), takes dog lease, summons; never summons a head a review covers. #65, #315
- Threads read until two reads agree (forge lists review before threads). #350
- Worker marks each bot thread `fixed` or `declined`; kelpie replies and resolves fixed ones, worker never posts. #367, `src/work_item/threads.rs`
- Merge yes accepts the threads it named. #455
- Bot passed over after far window, 2 silent hours or decline; re-read at green CI and every 5 min while merge ruling waits. #351, #399
- Open bot threads above nit block `auto`; nits named apart, never block. #350, #351
- Unheard summon resent once after 15 min, same lease. #122
- CodeRabbit gets `full review` on a PR it read or after catch-up (label sees only new commits). Clean catch-up with `main` summons no bot (own diff unchanged; CI checks combined code). #100, #106
- cubic summoned by comment (no label); Codex by `@codex review` or marking ready; `P0` to `P3` badges are severities. #124, #126
- Bot `rounds` caps reads per PR; adopted PR owes each bot a fresh summon. #84, #158

## CI and the merge

- After review item waits for CI; red = fix turn; second red on untouched head parks `still-red`; `ci.fix_attempts` caps fix turns (`-1` no cap). `ci.block` off reads no checks. #9, #380, `src/runner/gate.rs`, `src/settings/ci.rs`
- Branch behind `main` caught up by kelpie: rebase with lease, or merge where commits aren't kelpie's to rewrite. Conflict: kelpie starts `git merge --no-commit origin/main` in the worktree itself (so `main`'s files the sandbox refuses, `.claude/` included, land staged), and the worker turn only resolves the conflicted files, `git commit --no-edit`s and pushes without force. A rebase that conflicts falls back to that merge; a merge with no conflict is committed and pushed. A worktree already mid-merge is not merged again; unresolved parks `rebase`. #61, #609, `src/runner/gate/catch_up.rs`
- `git.merging` required: `auto` or `ask`. `ask`: yes merges only the head asked about while green and current, `--merge --match-head-commit`, never squash. `src/runner/merge.rs`
- `auto` replaces only the merge ruling; unreviewed passes, open threads, unread heads, unread fixes still raise it. Merge no longer valid returns to CI; second forge refusal parks `merge-refused`. One webhook notice per auto merge. #83, #118
- Merge queue holds the PR, kelpie skips gates meanwhile; removal goes back to worker like red CI. #213, `src/runner/merge/queue.rs`
- Head, label or ready state kelpie didn't set parks `foreign-change`; its yes gets a review pass. #69
- PR merged by hand ends item; closed unmerged parks `closed`. #377
- Rework reopens a kelpie PR on `ready-for-agent` or requested change (author can't request changes on own PR); fresh start, never force-push. #53
- Adoption: work item from a PR kelpie didn't open; merges `main` in, never rebases (owner may push). #84
- `ready-for-human` set at merge ruling, removed when work resumes. `src/runner/ruling.rs`
- Runner's own git/gh are plain children outside any worker fence. `src/spawn.rs`

## Rulings

- Six kinds: `merge`, `question`, `stuck` (with reason), `agent-files`, `foreign-change`, `follow-up` (kept: findings wait for a yes). ADR 0006, #330, `src/runner/ruling.rs`
- Ruling = log line + `status` entry + webhook post with one question; `no <note>` is worker's next turn. #89
- Ids are files linked into `rulings/<id>` (no repeat across projects). `rule <id> <answer>` by id or issue (`rule 427`; one ruling per item), refuses answer older than ruling. Each trigger is `shep kelpie <verb>`, project from folder unless `-p`. #119, #507
- `rule` reaches a stopped runner via `<kelpie home>/<project>/answers/<id>` files applied next pass. `src/runner/trigger.rs`
- One webhook (`discord` or `ntfy`) until shep carries questions; URL is a credential, passed to `curl` on stdin. ADR 0005, #333, `src/webhook.rs`
- ntfy reply carries TOTP (RFC 6238): a code posted on the alert sat on the topic it guarded. #111
- `shep kelpie totp`: secret under `<kelpie home>/totp/` (0700, 0600); every code seen in any post spent first; used steps hard-linked claims (runners agree); 5 wrong codes disable ntfy answers until `--unlock` at terminal. RFC 4226, #111
- Topic read every 15s while alerted ruling waits, backing off to 5 min. Limits fail closed: topic writer can lock answers or fake alerts, not act without code. #111
- `trusted` webhook with `token` needs codes only for merge yes. #298
