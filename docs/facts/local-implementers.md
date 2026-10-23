# Local implementers

- On the maintainer's live project (a Go and TypeScript web app), 22 kelpie pull requests merged between 2026-10-08 and 2026-10-09, nearly all built by a local implementer: Strata iq3_s through the gateway, at no Claude units for the worker.
- The nine local ones in the latest history: wall time 26 to 115 minutes per item, leaving out one held 11 hours on rulings; 2 to 8 worker calls; 3 to 5 review rounds; 1 to 5 fix turns.
- Two work items on one Strata model, which served one sequence at a time, evicted each other's cached prompt. Over 15 minutes, 28 of 31 requests read their whole prompt again, and that took 87% of the time (shep-pm/shep-paddock#17). So a project sharing one such server without a gateway keeps `concurrency.active_items` at 1.
- On the replay bench in the experiments repo, 2026-10-20 to 22: pi on Qwen `iq3_xxs`, 24 past kelpie issues it had failed, graded on hidden tests from the merged pull request
  - Lessons from earlier misses in the prompt: no change (p = 1.0)
  - A self-check closing step: +0.6 points of hidden tests over two reps, 3 tasks up
  - A second reader on the same local model listing gaps between the issue and the diff, then the worker continued with the list: about +3 points over two reps, 11 tasks up and 2 down, none solved. A Sonnet reader found the same gaps. It reads the issue literally, and once had the worker remove a case the shipped fix handles
  - Every question a triage pass asked, answered as the shipped code decided: 57% to 89% of hidden tests, solved 2 to 10. Only the top 3 answered: 69%, solved 4
