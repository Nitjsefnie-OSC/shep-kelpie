# Rulings

Rulings: kinds, ids, answers, the webhook.

- Six kinds: `merge`, `question`, `stuck` (with reason), `agent-files`, `foreign-change`, `follow-up` (kept: findings wait for a yes). ADR 0006, #330, `src/runner/ruling.rs`
- Ruling = log line + `status` entry + webhook post with one question; `no <note>` is worker's next turn. #89
- Ids are files linked into `rulings/<id>` (no repeat across projects). `rule <id> <answer>` by id or issue (`rule 427`; one ruling per item), refuses answer older than ruling. Each trigger is `shep kelpie <verb>`, project from folder unless `-p`. #119, #507
- `rule` reaches a stopped runner via `<kelpie home>/<project>/answers/<id>` files applied next pass. `src/runner/trigger.rs`
- One webhook (`discord` or `ntfy`) until shep carries questions; URL is a credential, passed to `curl` on stdin. ADR 0005, #333, `src/webhook.rs`
- ntfy reply carries TOTP (RFC 6238): a code posted on the alert sat on the topic it guarded. #111
- `shep kelpie totp`: secret under `<kelpie home>/totp/` (0700, 0600); every code seen in any post spent first; used steps hard-linked claims (runners agree); 5 wrong codes disable ntfy answers until `--unlock` at terminal. RFC 4226, #111
- Topic read every 15s while alerted ruling waits, backing off to 5 min. Limits fail closed: topic writer can lock answers or fake alerts, not act without code. #111
- `trusted` webhook with `token` needs codes only for merge yes. #298
