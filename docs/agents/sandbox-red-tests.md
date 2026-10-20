# Tests a worker's sandbox cannot run

A worker's sandbox binds no port by design (`a_worker_binds_no_port` in `src/runner/core_tests/profile.rs`), so some tests fail inside it and pass in CI. A red run of these in a worker is expected. It is not a bug in your change.

- Run what can run: `cargo test --lib <module path>` on the modules you changed.
- Name in the pull request body the tests only CI ran.
- Do not skip, `#[ignore]` or loosen a test to turn a sandbox run green.
- Make a test's temp folder with `crate::test::short_tempdir()`, never `tempdir_in("/tmp")`. In a crate under `crates/` it is `shep_kelpie_test_support::short_tempdir()`. It sits under `TMPDIR`, which a worker can write, and stays short enough for a socket. `scripts/no-tmp-tempdir.sh` fails CI on a new `tempdir_in("/tmp")` in `src/` or `crates/*/src/`.

## What fails, and why

The doctor, flock, dog (`left_over`), `lease::cli`, sheep, shepherd and upgrade tests run in a worker: they dial `FakeShepherd`, which answers in memory through the `Dial` port, and `lease::cli::e2e` reads the dog's answers off a connected pair of streams. What is listed below binds on purpose or for a reason of its own.

Measured with `cargo test --lib` in a worker, before #861: 150 of 3236 tests fail, 3069 pass, 17 are ignored. #861 adds tests that fail there for the reasons below (`adapters::dev_server`, the dev-origin tests of `adapters::bridge::proxy`, 5 of `runner::turn::tests::dev_server`) and has not re-measured the totals. A module's name covers its child modules. A listed module that lives in `crates/core` (`shepherd::connection`) runs under `cargo test -p shep-kelpie-core --lib`, and a bare `cargo test --lib` covers it too.

An entry whose parenthesis begins `some:` lists only part of the module: kelpie cannot tell which of its tests a worker cannot run. An entry with no such parenthesis, or one whose parenthesis says all, covers the module whole, and an entry listed whole in any row is whole. `kelpie_ci_failures_total` reads this table: only a whole entry makes a failing test in it `sandbox-only`, and a failing test in a `some:` entry counts as one the worker could have run. `SANDBOX_RED` in `src/runner/ci_failure.rs` holds the whole entries, and `the_doc_and_the_list_agree` fails when the two part ways.

| Reason | Modules |
|---|---|
| Unix socket bind refused (`Operation not permitted`) | `dog::door` (and `::e2e`), `adapters::test_runner`, `adapters::forwarder` (some: the forwarder's own socket), `adapters::pi::tests` (some: 3 tests that open a forwarder), `adapters::pi::attach_tests` (some: 3 tests that open a forwarder), `adapters::pi::adopt_tests` (some: 2 tests that carry a running call), `adapters::goose::tests` (some: 5 tests that open a forwarder or a stand-in paddock), `sheep::handover::tests::pi` (all 3: a stand-in paddock or a forwarder), `adapters::goose::adopt_tests` (some: 2 tests that carry a running call), `sheep::handover::tests::goose` (all 4: a stand-in paddock or a forwarder), `adapters::bridge` (some: 8 of 12 tests: those that open the browser's socket and its proxy, which binds a loopback TCP port), `adapters::bridge::proxy::tests` (some: all but the address check bind a loopback TCP port, the dev-origin tests included), `runner::turn::tests::browser` (some: 7 tests, those whose turn opens a bridge), `runner::turn::tests::dev_server` (some: 5 tests, those whose turn opens a bridge), `runner::flight::carry` (some: 1 test, a turn that holds a browser) |
| Unix socket bind in shep-client's fake shepherd (`shep-client/src/testing/shared.rs`), kept to prove a real socket | `dog::link` (some: 4 tests), `shepherd::connection` (some: 2 tests: the `Socket` dial reaching a shepherd, and its protocol-mismatch refusal), `tests/upgrade.rs` (the binary against a shepherd on a real socket) |
| Loopback TCP bind refused | `adapters::dev_server` (all of its tests, which find a free port), `adapters::local::endpoint`, `adapters::forwarder` (some: the stand-in model server in `crates/core/src/test/endpoint.rs`), `adapters::gpu`, `adapters::paddock`, `adapters::classifier` (some: 6 tests, the stand-in gateway its tests ask on the classifier's route), `adapters::curl`, `adapters::pi::gateway_tests`, `adapters::pi::attach_tests` (some: the forwarder test's stand-in model server), `adapters::goose::gateway_tests`, `adapters::goose::tests` (some: the stand-in paddock), `adapters::pi::adopt_tests` (some: the one test that starts a stand-in gateway), `adapters::goose::adopt_tests` (some: the one test that starts a stand-in gateway), `sheep::handover::tests::pi`, `sheep::handover::tests::goose`, `doctor::gateways`, `runner::gpu_tests`, `runner::limits_tests` (some: the paddock holder status test), `runner::pm::tests::lease` (some: 1 test, the lease wait test's stand-in model server), `runner::review::local`, `github` (some: 2 tests) |
| `ps` cannot say whether a process runs | `runner::attach`, `runner::flight::carry` (some: the 4 tests that attach a pi or goose session), `runner::pm::tests::attach`, `runner::slots_tests`, `runner::turn::tests::held` (some: the 2 tests that attach) |
| Process groups: a group check or signal the sandbox does not allow | `adapters::process` (some: 4 tests, the fourth the group an unserved handover ends), `adapters::dev_server` (some: the tests that start a server, whose group the guard stops), `adapters::claude::calls` (some: 1 test, the stop) |
| The temp folder is under `~/.shep`, which these tests' fences refuse to read | `issues::tests` (some: 2 tests), `runner::panel::tests::fences` (some: 2), `runner::pm::tests::code` (some: 1) |

## The tests behind `short_tempdir`

Of the sites that used `tempdir_in("/tmp")`:

- Pass in a worker: `adapters::claude::sandbox`, `adapters::codex`, and the rest of `adapters::pi::tests`.
- Pass by skipping: the two `adapters::srt::live` tests, which need macOS, kelpie's tools and a keychain, and `adapters::mcp::live`, which needs macOS, kelpie's tools with the code graph and a short `TMPDIR`. Each says why on stderr.
- Fail in a worker for a reason above: the 20 `adapters::forwarder::tests` that open a forwarder (Unix socket bind or the stand-in model server), and the 3 `adapters::pi::tests` that open a forwarder (Unix socket bind).
- `#[ignore]`d, and run by the `sandbox` CI job: `adapters::claude::escapes`.

## Keeping the list true

A change that moves a test into or out of this list updates the table in the same pull request. A new test that binds a socket or a port belongs in the table when it lands.

The table is also what `kelpie_ci_failures_total` counts a red run on: `SANDBOX_RED` in `src/runner/ci_failure.rs` holds the table's whole entries, and `the_doc_and_the_list_agree` fails when the two part ways. A whole entry added to the table and not to the list, or the other way round, is a failing test, not a note. A `some:` entry belongs in the table and not in the list: it says the module is red in part, which is not enough to call a failing test in it one a worker could not run.

The list is of this crate's modules, so kelpie uses it only on this repo. On a project kelpie holds no list of, every failing test of a red run counts as one its worker could have run, and `sandbox-only` is never used there. Giving another project a list of its own is unbuilt.
