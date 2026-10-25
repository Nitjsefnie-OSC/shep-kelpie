# Windows

Kelpie is being ported (#241). This area holds the shell-outs and the tests' stand-in commands (#1149).

- `spawn::sh()` and `spawn::env()` are the only way to start a shell or `env`. On Unix they are `/bin/sh` and `/usr/bin/env`. On Windows they find Git for Windows through a `git.exe` on `PATH`, then its default install folders, and fall back to the bare name.
- A hook, the test-runner, the dev server's install and the Claude sandbox's `env` run the maintainer's POSIX shell line, so they require Git for Windows.
- `tar` and `shasum` are reached only for a release asset, which has no Windows build, so `upgrade` keeps them Unix. `kill` and `ps` wait on #1148, sockets on #1147.
- Workers run through crabpot here as on every platform (#1217): the wrap is kelpie's own binary with `--crabpot`, and the machine needs a one-time `crabpot windows-install`, which fixes the loopback mode of every session after it. `crabpot::windows_status` reads what the install says, `Host::windows_status` carries it to `doctor`, and `docs/design/fence.md` holds the loopback rule.
- A test's stand-in command is `write_fake` and `Fake` from `test_support`: an `sh` script on Unix, a `.cmd` file on Windows. `write_script` is for a script that needs real shell, which Windows runs through `spawn::sh()`.
- A test that needs a FIFO is `#[cfg_attr(windows, ignore = "a FIFO is a Unix file")]`.
- `docs/agents/sandbox-red-tests.md` lists what Windows cannot run, read from the source, since the Windows job only runs `cargo check`.
- `core` and the crates above it do not compile for Windows yet, so only `process` and `test_support` are checked against the Windows target.
