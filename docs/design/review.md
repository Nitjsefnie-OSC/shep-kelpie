# Review

Review passes, bots, rulings. CI and the merge are in merge.md.

## Review

- One pass when CI green on draft head: `agents.reviewers` in order, each once; no loop or judge (gains come in the first two reads). ADR 0006, ADR 0008, `src/runner/review/`
- Any finding, nits included, goes to one fix turn; next reviewer reads the fix (skipped nits left open threads). Nit-only fix starts nothing new; bot nits on a head it read stay open. ADR 0007, #354
- Round waits for CI on its head first (red costs no read or window); `doctor` names CI that skips drafts. #423
- A contribution's head gets its held runs approved only after it passes the security review, and its CI is tracked in `contributions[].ci`; no reviewer reads it before `green` (ADR 0008), and then one pass of the contribution's reviewers records findings, posting none. `contributions.md`. #622, #623
- Pass restarts only on a new head that needs it (merge `rework`, accepted foreign change, rework). ADR 0006
- Merge ruling `no <note>` = fix turn back to the ruling, no new pass; restarts pass only after unread/open-thread warning. #354
- Reviewer = agent file on session harness (read-only), `command`, `endpoint` or `bot`; runs no commands (running tests found fewer bugs). #314
- `paths` limit a reviewer to matching changes; every round reads `<base>...HEAD`. Gets issue title + acceptance criteria (4,000 bytes), so no whole-issue check. #314, #484
- `defect-hunter` (Opus 5.5 high, maintainer's prompt verbatim) uses `second_look`. `add` lists `["qwen", "defect-hunter"]` where qwen script exists, else `["defect-hunter"]`. `agents/defect-hunter.md`, `src/flock/verbs.rs`
- `standards` and `spec` (Opus 5.5 high, one read, no `second_look`) are the two axes of the vendored `code-review` skill, one agent each since parallel sub-agents are forbidden. Smells are `LOW`; `add` writes both and lists neither, and a project's `reviewers` list replaces the default, so it names them with `defect-hunter`. They sit beside `defect-hunter`, which finds behaviour defects they do not. `agents/standards.md`, `agents/spec.md`, `src/agents.rs`. #363
- Reviewer failing 3 times passed over; pass nobody read marks item unreviewed (API outage can't auto-merge). #314
- Round reads only pushed head; dirty worktree gets one turn to push, then parks `unpushed` (CI and merge take `origin`'s head). #290
- Merge trusts only heads a round read or kelpie sent to CI unread on purpose; others raise `unread_head`. #290
- A maintainer's yes to a merge ruling vouches for its head, so kelpie's catch-up with `main` before the merge carries the vouching and raises no second ruling; a no or a rework vouches for nothing. #668
- Fix turn counts only if it moves `origin` head, unless it deferred all or got only nits; else parks `fix-not-pushed`. #40, #354
- Clean reply is exactly `CLEAN`. #314
- Local `command`/`endpoint` reviewers per project, checked at start; `endpoint` chunks diff by `context`; `command` keeps qwen script contract. #107
- Local reviewer holds no lock (`lease` refused, no `/api/ps` check, no `local-model-spilled`); running round never preempted (kill wastes the work). #145, #502
- One finding shape for every reviewer kind; bots are one provider with a tested profile each. #123
- With `ci.test_command`, a reviewer on Claude Code (`Write`) or pi (`write`, held by the guard extension's `kelpie confine`) may attach a failing test, held to `review-tests` in the build folder; Codex is not asked. The test runs before/after fix and is recorded. Still failing after the fix: worker gets one more fix turn on it (`sent_back` on the check), then `stuck` `test-still-fails`; yes = one more turn, no = check ends and pass goes on. Timeout or not run never acts. Public line names no output. #517, #518, #519
- Deferred finding kelpie sent is filed after merge in kelpie's words (nits dropped). `git.issues`: `ask` (default, `follow-up` ruling), `file` (as `needs-triage`; auto-filed were often false), `skip`. #142, #380, #384, `src/runner/follow_up.rs`

## Bots

- Bots = `harness: bot` reviewer files named for the bot (its account owns the window): CodeRabbit 1 in 1h, cubic 20 in 720h, Codex 10 in 168h. #315
- Bot round marks draft ready (CodeRabbit skips drafts), takes dog lease, summons; never summons a head a review covers. #65, #315
- Threads read until two reads agree (forge lists review before threads). #350
- Worker marks each bot thread `fixed` or `declined`; kelpie replies and resolves fixed ones, worker never posts. #367, `src/work_item/threads.rs`
- Merge yes accepts the threads it named. #455
- Writer comments and review threads (owner, member, collaborator; other Apps ignored and logged) read at round start, green CI and while a merge ruling waits, batched into one noted turn; ambiguous ones become a `question` ruling; replies as #367; a comment while a merge ruling waits withdraws it as `no <note>`; 3 rounds per item then a `question` ruling. #368, `src/runner/talk.rs`
- Bot passed over after far window, 2 silent hours or decline; re-read at green CI and every 5 min while merge ruling waits. #351, #399
- Open bot threads above nit block `auto`; nits named apart, never block. #350, #351
- Unheard summon resent once after 15 min, same lease. #122
- CodeRabbit gets `full review` on a PR it read or after catch-up (label sees only new commits). Clean catch-up with `main` summons no bot (own diff unchanged; CI checks combined code). #100, #106
- cubic summoned by comment (no label); Codex by `@codex review` or marking ready; `P0` to `P3` badges are severities. #124, #126
- Bot `rounds` caps reads per PR; adopted PR owes each bot a fresh summon. #84, #158

## Rulings

- Seven kinds: `merge`, `question`, `stuck` (with reason), `agent-files`, `foreign-change`, `follow-up` (kept: findings wait for a yes), `contribution` (with reason; `security` for a flagged head, see contributions.md). ADR 0006, #330, `src/runner/ruling.rs`
- A `contribution` ruling has no work item and is never posted on the contributor's pull request (it would accuse them and tip off an attacker): webhook, `status` and `rule` only. It never goes to the panel, and `rulings.pm_alone` naming `security` is refused at load. #620
- Ruling = log line + `status` entry + webhook post with one question; `no <note>` is worker's next turn. #89
- A ruling is named by its number where the maintainer reads it: `#<pr>`, or `#<issue>` before there is a pull request. The question, the webhook title, kelpie's GitHub comment and card and `status`'s `id` all show it, and a reply starts with it. The number is the ruling's only name: the counter ids are gone, and a state file saved under them is renamed when it loads (version 38). #508, #509
- `rule <number> <answer>` (`rule 427`; one ruling per item within a project), refuses an answer older than the ruling. An answer file left under an old counter id is read once, then refused. #509. Each trigger is `shep kelpie <verb>`, project from folder unless `-p`. #119, #507
- `rule` reaches a stopped runner via `<kelpie home>/<project>/answers/<number>` files applied next pass. `src/runner/trigger.rs`
- One webhook (`discord` or `ntfy`) until shep carries questions; URL is a credential, passed to `curl` on stdin. ADR 0005, #333, `src/webhook.rs`
- ntfy reply carries TOTP (RFC 6238): a code posted on the alert sat on the topic it guarded. #111
- `shep kelpie totp`: secret under `<kelpie home>/totp/` (0700, 0600); every code seen in any post spent first; used steps hard-linked claims (runners agree); 5 wrong codes disable ntfy answers until `--unlock` at terminal. RFC 4226, #111
- Topic read every 15s while alerted ruling waits, backing off to 5 min. Limits fail closed: topic writer can lock answers or fake alerts, not act without code. #111
- `trusted` webhook with `token` needs codes only for merge yes. #298
