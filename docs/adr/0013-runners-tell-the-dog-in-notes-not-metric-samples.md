# Runners tell the dog in notes, not metric samples

This reopens one line of ADR 0002: "triggers down, channel metrics up on the bus". Runners still report running totals up the bus, but as notes.

A runner told the dog about its leases and review windows with channel metric samples: `lease.<kind>.want.<epoch>`, `lease.<kind>.return.<epoch>` and `window.<kind>.<fact>.<epoch>`. Shep's metrics dog reads every sample on the channel, and refuses each of these names as `invalid_name`. On the maintainer's shepherd that came to 662 refusals in half an hour, all from one runner (#931). The names were never metrics: they are messages for one dog that happen to carry a number.

shep 0.15.0 has a message for that (shep-pm/shep#913, shipped in shep-pm/shep#914): a note, `{"kind":"note","name":...,"value":...}` on the channel, sent with `Shepherd::note` and published on the bus as `channel.note`. The metrics dog does not subscribe to it, and shep puts no grammar on its name.

The decision:

- Runners send their lease and window totals as notes alone, under the same names and values (`crates/core/src/lease/wire.rs`, `src/adapters/leases.rs`). Like a sample, a note never blocks and may be dropped under backpressure, so the totals stay running totals.
- The dog subscribes to `channel.note` and `channel.metric`, and reads both into the same desk (`src/dog/link.rs`, `src/dog/mod.rs`). An upgrade restarts the dog before its runners, so for one release a runner still on the old build keeps being granted. A later release removes the `channel.metric` path.
- Kelpie requires shep 0.15.0. Its shep version check refuses an older shepherd, which would drop every note as malformed.

## Considered options

- **Keep samples and have shep's metrics dog skip kelpie's names.** The metrics dog would have to know another dog's names, and every other sheep's malformed samples would still need the refusal.
- **Rename the totals to names the metrics dog takes.** They would become series in the maintainer's metrics store, one per run and kind, for numbers nobody graphs.
- **A socket of kelpie's own between runner and dog.** The runner already has the shepherd channel, and shep already carries the bus across a shepherd restart. Notes are the general feature (CLAUDE.md, "Kelpie and shep").

## Consequences

- A dog from a build before this one hears no runner on this build. `shep kelpie upgrade` restarts the dog first, so an upgrade keeps granting, but running a new runner beside an old dog does not.
- Numbers kelpie sends for the maintainer's graphs stay metric samples. Notes are only for what the dog reads, so they never reach the metrics store.
