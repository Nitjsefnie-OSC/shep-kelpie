# The getting-started path

- Measured 2026-09-30 on a scratch shepherd (shep 0.12.0) with `SHEP_HOME` and `KELPIE_HOME` both pointing into a scratch folder, on a clone of shep-pm/shep:
  - `cargo install --git https://github.com/shep-pm/shep-kelpie --locked` builds in about 77s and installs one binary, `shep-kelpie`. `shep adopt shep-kelpie --name kelpie` finds it on `PATH`.
  - Nothing runs until a shepherd does: `shep flock` and `shep dogs` both answer "no shepherd is running" before `shep muster`, which starts one and the adopted dog with it, online within seconds.
  - `shep kelpie tools install` downloaded about 95 MiB of headless Chrome and 1 MiB of ffmpeg into `<kelpie home>/tools`, for shots; since shots were parked (ADR 0006) it installs the sandbox runtime alone.
- The dog wrote no book under the scratch `KELPIE_HOME` or under `~/.kelpie` while idle, so whether a shepherd started from a shell with `KELPIE_HOME` set hands it to the adopted dog is still open. The scratch run never touched a real book.
- `shep kelpie add` refuses a checkout with no `origin`, and a repo GitHub cannot resolve. A project's `doctor` passes a fresh install with no webhook, and its rulings line says rulings then reach the maintainer only in the log, `status` and `shep kelpie rule`.
- Not walked live: `start`, a first labelled issue and a ruling, since a runner on a clone of shep-pm/shep would pick up that repo's own `ready-for-agent` issues.
