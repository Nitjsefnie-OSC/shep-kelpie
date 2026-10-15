# Kelpie's numbers go out on the shepherd channel

Kelpie's features have failed for days with nothing to show it. The code graph never connected in a worker call from Oct 13 to Oct 15 (#918), and a worker's `shadcn` MCP server failed the same way on #135. The signals were there, in a note told once, a `doctor` line and the transcripts, but each needed someone to go and look. What kelpie spends and how its work items go was read by scripts on the maintainer's machine, which no other user has.

shep 0.14 lets any sheep or dog send a metric sample, with a type, labels and help, on its shepherd channel (shep-pm/shep#717). The metrics dog keeps the last sample per sender and series until the sender stops or restarts, and serves them on its Prometheus endpoint beside the flock's own series, and in its OTLP export.

The decision: the runner and the dog send kelpie's numbers as channel samples, named `kelpie_*`. Counters are kept in kelpie's memory and sent whole, so a dropped sample costs nothing. Labels are bounded sets: role, call kind, harness, model, how a call ended, phase, feature, outcome. Never an issue, a pull request or a session. Detail by work item stays in kelpie's ledger and is read with `shep kelpie` commands, which any user can run and paste into a bug. Kelpie ships the alert rules and a Grafana dashboard for its series. The design is in `docs/design/metrics.md`.

## Considered options

- **Kelpie serves its own `/metrics`.** A second endpoint per runner, a port each sandbox and firewall has to know, and a second place to scrape. Shep's endpoint already carries the flock, and shep-pm/shep#717 was built for this.
- **Scripts over the ledger and transcripts.** What kelpie had. They live on one machine, nobody runs them until something already looks wrong, and they cannot alert.
- **A label per work item.** The metrics dog holds at most 256 series a sender, and a series per issue grows without end. The ledger already has every call by issue.
- **OTLP from kelpie.** The metrics dog exports OTLP already, samples included (shep-pm/shep#736).

## Consequences

- A runner's series go when it stops, and its counters start again from zero when it starts. Prometheus keeps the history, and `rate` and `increase` read across a restart.
- There is no histogram type on the channel, so a distribution goes out as counters with an `le` label, which `histogram_quantile` reads.
- The lease totals and summons a runner sends the dog (`lease.<kind>.want.<epoch>`, `window.<reviewer>.summoned.<pid>`) are not Prometheus names. As channel samples, the metrics dog refused each one and counted it in `shep_channel_samples_refused_total{reason="invalid_name"}`, which buried any real bad name. #931 sends them as notes (shep's `channel.note`, shep-pm/shep#913), which the metrics dog never reads.
- A feature that is on but never works shows as a series that never moves, and the shipped alert rule fires on it.
