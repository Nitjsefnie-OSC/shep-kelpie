# The keychain in a sandboxed call

Measured 2026-10-10 on `@anthropic-ai/sandbox-runtime` 0.0.77, macOS, with a live worker's settings and its writable paths moved to a test folder (shep-pm/shep-kelpie#429).

Re-read 2026-10-23 on 0.0.81 (shep-pm/shep-kelpie#1180): the compiled profile names each service once, `com.apple.securityd.xpc` at `dist/sandbox/macos-sandbox-utils.js:785` and `com.apple.SecurityServer` at `:907`, so the cut holds unchanged. The probe table below is from 0.0.77 and was not re-run by hand. `adapters::srt::live` re-runs the keychain probes on macOS in CI.

- `srt` writes two keychain services into every macOS profile: `com.apple.securityd.xpc` (`dist/sandbox/macos-sandbox-utils.js:706` on 0.0.77) and `com.apple.SecurityServer` (`:828`). Its settings can add Mach services but not remove them. So any command a worker runs can ask for a login that the `security` command stored, without a prompt. A file rule cannot stop that, and a scoped token does not help while the maintainer's own login is one call away.
- "Blocked" is a copy of `srt` with those two lines removed:

  | Probe | srt as shipped | Blocked |
  |---|---|---|
  | `security find-generic-password -s gh:github.com` | found | refused |
  | `security find-generic-password -s "Claude Code-credentials"` | found | refused |
  | `git credential-osxkeychain get` for github.com | password | refused |
  | `git ls-remote https://github.com/...` | ok | ok |
  | `curl https://api.github.com` | ok | ok |
  | `gh api` with a token (Go TLS) | ok | ok |
  | python `urllib` https | ok | ok |
  | `cargo search`, `cargo new && cargo run`, `codesign --sign -` | ok | ok |
  | `claude -p` | ok | fails: its login is in the keychain |

- A python process calling the Security framework directly was not silent: macOS asked the maintainer for their password. Items stored through the `security` command are the ones handed out without a prompt, and `gh`'s, git's and Claude Code's all are.
- So kelpie makes the blocked copy at `tools install` (`<tools>/node_modules/@anthropic-ai/sandbox-runtime-no-keychain`), cutting the one line that names each service and refusing a runtime that names either on any other number of lines. A call on a repo the App covers runs under the copy and refuses to start when it is missing or names a service. A repo the App does not cover runs under the runtime as shipped, since its worker pushes and runs `gh` with the keychain's login.
- A blocked `claude` logs in with `CLAUDE_CODE_OAUTH_TOKEN`, made once with `claude setup-token` and kept with `shep secret`. The runner hands it to `claude` alone. Every other process it starts has the variable unset, and the `/usage` read, which runs outside the sandbox, keeps using the keychain login.
- `srt` has no setting for it. Proposing a `denyMachLookup` or a keychain switch to `srt` itself can follow, and the patch goes then.
