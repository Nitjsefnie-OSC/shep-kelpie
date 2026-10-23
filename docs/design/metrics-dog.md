# Metrics: the dog's series

The dog sends through the same `Recorder` as a runner (`crates/core/src/metrics/mod.rs`); the rest of the metrics design is in `metrics.md`. ADR 0012, #920.

## Built

- It sends on its own `Shepherd` (`crates/core/src/metrics/dog.rs`), labelled `dog` and `source` by the metrics dog.
- `kelpie_leases_held{kind}`, `kelpie_lease_waiting{kind}` gauges, `kelpie_lease_grants_total{kind}` and `kelpie_lease_wait_seconds_total{kind}`: the desk looks at every lease after each change (`Desk::look`, `dog/watch/`). A grant is a holder not there at the last look, so the book's, the maintainer's and `cargo-test`'s are counted by one rule; the wait is from when the dog first saw the waiter queued, and a waiter that leaves is not counted. `e2e` is one kind for every host, as a host name is not a bounded label. A command seated for a run under the last dog is held but not a grant.
- `kelpie_summons_total{bot,outcome}`: `accepted` or `refused`, counted once per bot and time (or quoted opening) by the watch, not the window, so the runners' repeated totals count once, also after the window forgot a summon or the dog restarted (the saved windows seed the watch). A summon older than its span, or a refusal whose opening has passed, is not news when heard. The maintainer's grants count as summons in the window, not here.
- `kelpie_forge_holds_total{login_kind}` and `kelpie_forge_held`: a hold begins when a runner reports one and none stands. `login_kind` is always `login`: the dog's hold is the login's, as the App's used-up answer holds only its own reads. The hold ends on its own, so the dog's tick (`Kept::tick`) sets the gauge and sends every series again each minute.
- The login's rate limit gauges, `kelpie_forge_rate_used` and `kelpie_forge_rate_reset_timestamp_seconds`, come from the runners and not the dog: the dog has no `gh`, and a runner already asks the forge. Every runner reports the same login's numbers, so a query takes `max without (sheep, id, fold)` across them (`docs/guide/metrics.md`).
