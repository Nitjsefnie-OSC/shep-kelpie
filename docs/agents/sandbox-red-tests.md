# Tests a worker's sandbox cannot run

A worker's sandbox binds no port by design (`a_worker_binds_no_port` in `src/profile.rs`), so some tests fail inside it and pass in CI. A red run of these in a worker is expected. It is not a bug in your change.

- Run what can run: `cargo test --lib <module path>` on the modules you changed.
- Name in the pull request body the tests only CI ran.
- Do not skip, `#[ignore]` or loosen a test to turn a sandbox run green.
- Make a test's temp folder with `crate::test::short_tempdir()`, never `tempdir_in("/tmp")`. It sits under `TMPDIR`, which a worker can write, and stays short enough for a socket. `scripts/no-tmp-tempdir.sh` fails CI on a new `tempdir_in("/tmp")` in `src/`.

## What fails, and why

The doctor, flock, dog (`left_over`), `lease::cli`, sheep, shepherd and upgrade tests run in a worker: they dial `FakeShepherd`, which answers in memory through the `Dial` port, and `lease::cli::e2e` reads the dog's answers off a connected pair of streams. What is listed below binds on purpose or for a reason of its own.

Measured with `cargo test --lib` in a worker: 144 of 3225 tests fail, 3064 pass, 17 are ignored. A module's name covers its child modules.

| Reason | Modules |
|---|---|
| Unix socket bind refused (`Operation not permitted`) | `dog::door` (and `::e2e`), `adapters::test_runner`, `adapters::forwarder` (the forwarder's own socket), `adapters::pi::tests` (3 tests that open a forwarder) |
| Unix socket bind in shep-client's fake shepherd (`shep-client/src/testing/shared.rs`), kept to prove a real socket | `dog::link` (4 tests), `shepherd::connection` (2 tests: the `Socket` dial reaching a shepherd, and its protocol-mismatch refusal), `tests/upgrade.rs` (the binary against a shepherd on a real socket) |
| Loopback TCP bind refused | `adapters::local::endpoint`, `adapters::forwarder` (the stand-in model server in `src/test/endpoint.rs`), `adapters::gpu`, `adapters::paddock`, `adapters::curl`, `adapters::pi::gateway_tests`, `doctor::gateways`, `runner::gpu_tests`, `runner::review::local`, `github` (2 tests) |
| `ps` cannot say whether a process runs | `runner::attach`, `runner::pm::tests::attach`, `runner::slots_tests`, `runner::turn::tests::held` (the 2 tests that attach) |
| Process groups: a group check or signal the sandbox does not allow | `adapters::process` (3 tests), `adapters::local::queue` (2), `adapters::local::watched` (1) |
| The temp folder is under `~/.shep`, which these tests' fences refuse to read | `issues::tests` (2 tests), `runner::panel::tests::fences` (2), `runner::pm::tests::code` (1) |

## The tests behind `short_tempdir`

Of the sites that used `tempdir_in("/tmp")`:

- Pass in a worker: `adapters::claude::sandbox`, `adapters::codex`, and the rest of `adapters::pi::tests`.
- Pass by skipping: the two `adapters::srt::live` tests, which need macOS, kelpie's tools and a keychain, and say so on stderr.
- Fail in a worker for a reason above: the 20 `adapters::forwarder::tests` that open a forwarder (Unix socket bind or the stand-in model server), and the 3 `adapters::pi::tests` that open a forwarder (Unix socket bind).
- `#[ignore]`d, and run by the `sandbox` CI job: `adapters::claude::escapes`.

## Keeping the list true

A change that moves a test into or out of this list updates the table in the same pull request. A new test that binds a socket or a port belongs in the table when it lands.
