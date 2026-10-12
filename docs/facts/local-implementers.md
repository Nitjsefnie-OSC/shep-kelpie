# Local implementers

- On the maintainer's live project (a Go and TypeScript web app), 22 kelpie pull requests merged between 2026-10-08 and 2026-10-09, nearly all built by a local implementer: Strata iq3_s through the gateway, at no Claude units for the worker.
- The nine local ones in the latest history: wall time 26 to 115 minutes per item, leaving out one held 11 hours on rulings; 2 to 8 worker calls; 3 to 5 review rounds; 1 to 5 fix turns.
- Two work items on one Strata model, which served one sequence at a time, evicted each other's cached prompt. Over 15 minutes, 28 of 31 requests read their whole prompt again, and that took 87% of the time (shep-pm/shep-paddock#17). So a project sharing one such server without a gateway keeps `concurrency.active_items` at 1.
