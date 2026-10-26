# Windows

Kelpie is being ported (#241). This area holds the shell-outs and the tests' stand-in commands (#1149).

- `spawn::sh()` and `spawn::env()` are the only way to start a shell or `env`. On Unix they are `/bin/sh` and `/usr/bin/env`. On Windows they find Git for Windows through a `git.exe` on `PATH`, then its default install folders, and fall back to the bare name.
- A hook, the test-runner, the dev server's install and the Claude sandbox's `env` run the maintainer's POSIX shell line, so they require Git for Windows.
- `tar` and `shasum` are reached only for a release asset, which has no Windows build, so `upgrade` keeps them Unix. `kill` and `ps` wait on #1148, sockets on #1147.
- Workers run through crabpot here as on every platform (#1217): the wrap is kelpie's own binary with `--crabpot`, and the machine needs a one-time `crabpot windows-install`, which fixes the loopback mode of every session after it. `crabpot::windows_status` reads what the install says, `Host::windows_status` carries it to `doctor`, and `docs/design/fence.md` holds the loopback rule.
- A test's stand-in command is `write_fake` and `Fake` from `test_support`: an `sh` script on Unix, a `.cmd` file on Windows. `write_script` is for a script that needs real shell, which Windows runs through `spawn::sh()`.
- A test that needs a FIFO is `#[cfg_attr(windows, ignore = "a FIFO is a Unix file")]`.
- `docs/agents/sandbox-red-tests.md` lists what Windows cannot run, read from the source, since the Windows job only runs `cargo check`.
- `core` and the crates below it compile for Windows, tests included (#1262), and the `windows` job's gating `cargo check` covers them. `dog` compiles for Windows too (#1274), and its check is gating. `adapters` compiles too, tests included (#1298). `runner` compiles too, tests included (#1299). The root package does not yet (#241).
- A file's mode is a Unix answer, and `core` gives Windows one of its own where it can (#1262):
  - the home stamp reads `Metadata::modified`.
  - the fence takes no executable bit from a Windows disk, since git there keeps none: a fenced file matches either regular mode of the commit.
  - `totp::private_dir` makes the folder with `fs::create_dir_all`, since the runner calls it to mint the App's token.
  - the TOTP secret and the answers files are refused with `unsupported`, not made with looser access, until they get an owner-only ACL (#1264).
- A test that reads or sets a mode, or makes a symlink, is `#[cfg(unix)]`, since its body names a Unix API. One that only needs a Unix script or secret is `#[cfg_attr(windows, ignore = "...")]`.
- The dog's e2e run compiles for Windows, but copying its results into the worktree is refused with `unsupported` (#1302): it opens each folder from the one before with `O_NOFOLLOW`, which Windows has no call for. The tests that run its `ssh` and `rsync` stand-ins, which are `sh` scripts, are `#[cfg_attr(windows, ignore)]` until then. A test that makes a symlink is `#[cfg(unix)]`.
- The adapters crate gives Windows these answers (#1298):
  - the App store keeps and reads no key until #1264: `Apps::save` and a `get` of an App on record say so, naming #1264, and write nothing. No record is still `Ok(None)`, so a repo with no App keeps the `gh` login.
  - `StatDisk` says `Unsupported` for both readings until #1300. `runner::builds` turns that into a note and applies no limit.
  - `adapters::files::is_executable` is the mode bits on Unix and a regular file on Windows, and `on_path` also tries each `PATHEXT` extension there.
  - `adapters::files::link` is `symlink` on Unix, and `symlink_file` or `symlink_dir` by the target on Windows, which needs Developer Mode (#1271). The Codex login and both skill folders use it.
  - `adapters::mcp::live`, which binds a Unix socket named by a uid, is built on Unix only.
- The runner crate gives Windows these answers (#1299):
  - `code_graph::too_long` says `None`, since the daemon's `cbm-daemon-<uid>` folder and the socket path limit it explains are Unix ones.
  - a test that sets a mode or makes a symlink is `#[cfg(unix)]`. The rig's `hook` and `write_totp_secret` set no mode on Windows.
