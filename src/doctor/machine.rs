//! The checks that hold for every project: `claude`, `gh`, the sandbox and the shepherd

use std::path::Path;

use shep_client::shep_core::protocol::request::ProcessInfo;

use super::Line;
use super::host::{Host, at_least};
use crate::adapters::{GOOSE_MINIMUM, LOGIN_FIX, LOGIN_VAR, PI_MINIMUM};
use crate::ports::{Clock, Forge, ForgeError, Meter, MeterError};
use crate::shepherd::{ConnectRefused, Connection};
use crate::tools::Tools;

// A tool's own message can run to a page, and the first line says what went wrong.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no message")
}

fn says_logged_out(said: &str) -> bool {
    let said = said.to_lowercase();
    said.contains("/login") || said.contains("logged in")
}

/// `claude` installed and logged in:`/usage` answers only for an account
pub(super) fn claude(meter: &dyn Meter, clock: &dyn Clock) -> Line {
    match meter.read(clock.now()) {
        Ok(_) => Line::ok("claude", "installed and logged in"),
        Err(MeterError::Spawn(why)) => Line::missing(
            "claude",
            format!("cannot run `claude`: {why}"),
            "install Claude Code (https://claude.com/claude-code) and put `claude` on your PATH",
        ),
        Err(MeterError::Unreadable(said)) if says_logged_out(&said) => Line::missing(
            "claude",
            format!(
                "`claude` runs but gave no usage, which it does only when logged in: {}",
                first_line(&said)
            ),
            "run `claude`, then `/login`",
        ),
        Err(MeterError::Unreadable(said)) => Line::unsure(
            "claude",
            format!(
                "`claude` runs, but its usage was unreadable: {}",
                first_line(&said)
            ),
            "run `claude -p /usage` to see what it says",
        ),
        Err(e @ (MeterError::TimedOut | MeterError::Stopped | MeterError::Codex(_))) => {
            Line::unsure(
                "claude",
                e.to_string(),
                "run `claude -p /usage` to see what it says",
            )
        }
    }
}

/// `codex` installed and logged in to an account that answers its usage,
/// for `subject`, a project that spends the Codex account
pub(super) fn codex(subject: String, meter: &dyn Meter, clock: &dyn Clock) -> Line {
    let reason = match meter.read(clock.now()) {
        Ok(usage) => {
            let (session, week) = (usage.session.used_pct, usage.week.used_pct);
            return Line::ok(
                subject,
                format!("reads Codex usage: 5-hour window {session}%, week {week}%"),
            );
        }
        Err(e) => e.to_string(),
    };
    if reason.starts_with("cannot run codex") {
        Line::missing(
            subject,
            reason,
            "install the Codex CLI and put `codex` on your PATH",
        )
    } else if reason.starts_with("codex refused") {
        Line::missing(
            subject,
            reason,
            "run `codex login` as an account whose plan includes Codex",
        )
    } else {
        Line::unsure(
            subject,
            reason,
            "run `codex` and its `/status` to see what it says",
        )
    }
}

/// `pi` on the `PATH` at [`PI_MINIMUM`] or later, for `subject`, a project with an agent on pi
pub(super) fn pi(subject: String, host: &dyn Host) -> Line {
    if !host.on_path("pi") {
        return Line::missing(
            subject,
            "cannot run `pi`: it is not on your PATH",
            "run `brew install pi-coding-agent`",
        );
    }
    let said = match host.program_version("pi") {
        Ok(said) => said,
        Err(why) => {
            return Line::unsure(
                subject,
                format!("`pi --version` gave no version: {why}"),
                "run `pi --version`",
            );
        }
    };
    match at_least(&said, PI_MINIMUM) {
        Some(true) => Line::ok(
            subject,
            format!("pi {said}, kelpie needs {PI_MINIMUM} or later"),
        ),
        Some(false) => Line::missing(
            subject,
            format!("pi {said} is older than the {PI_MINIMUM} kelpie needs"),
            "run `brew upgrade pi-coding-agent`",
        ),
        None => Line::unsure(
            subject,
            format!(
                "`pi --version` printed `{}`, which is not a version number",
                first_line(&said)
            ),
            "run `pi --version`",
        ),
    }
}

// Each of pi's own search tools, the program it runs, and every name pi
// accepts for it: Debian calls `fd` `fdfind`.
const PI_TOOLS: [(&str, &str, &[&str]); 2] =
    [("grep", "rg", &["rg"]), ("find", "fd", &["fd", "fdfind"])];

/// `rg` and `fd` on the `PATH`, which pi's own `grep` and `find` tools run,
/// for `subject`, a project with an agent on pi
///
/// pi looks in its own `bin` folder first, which is empty for every kelpie
/// call since each call gets a pi home of its own, then on `PATH`, then
/// downloads the program from GitHub, which the sandbox refuses. A tool with
/// none of those fails in every call (`docs/facts/tool-errors.md`).
pub(super) fn pi_tools(subject: String, host: &dyn Host) -> Line {
    let gone: Vec<(&str, &str)> = PI_TOOLS
        .iter()
        .filter(|(_, _, names)| !names.iter().any(|name| host.on_path(name)))
        .map(|(tool, program, _)| (*tool, *program))
        .collect();
    if gone.is_empty() {
        return Line::ok(
            subject,
            "`rg` and `fd` are on your PATH, which pi's `grep` and `find` run",
        );
    }
    let said: Vec<String> = gone
        .iter()
        .map(|(tool, program)| {
            format!(
                "`{tool}` runs `{program}`, which is not on your PATH, so pi's `{tool}` tool fails in every call"
            )
        })
        .collect();
    Line::missing(
        subject,
        said.join("; "),
        "install them: `brew install ripgrep fd` on macOS, `apt-get install ripgrep fd-find` on Debian",
    )
}

/// `goose` on the `PATH` at [`GOOSE_MINIMUM`] or later, for `subject`, a project with an agent on goose
pub(super) fn goose(subject: String, host: &dyn Host) -> Line {
    if !host.on_path("goose") {
        return Line::missing(
            subject,
            "cannot run `goose`: it is not on your PATH",
            "run `brew install block-goose-cli`",
        );
    }
    let said = match host.program_version("goose") {
        Ok(said) => said,
        Err(why) => {
            return Line::unsure(
                subject,
                format!("`goose --version` gave no version: {why}"),
                "run `goose --version`",
            );
        }
    };
    match at_least(&said, GOOSE_MINIMUM) {
        Some(true) => Line::ok(
            subject,
            format!("goose {said}, kelpie needs {GOOSE_MINIMUM} or later"),
        ),
        Some(false) => Line::missing(
            subject,
            format!("goose {said} is older than the {GOOSE_MINIMUM} kelpie needs"),
            "run `brew upgrade block-goose-cli`",
        ),
        None => Line::unsure(
            subject,
            format!(
                "`goose --version` printed `{}`, which is not a version number",
                first_line(&said)
            ),
            "run `goose --version`",
        ),
    }
}

/// `gh` installed and logged in
pub(super) fn gh(forge: &dyn Forge) -> Line {
    match forge.viewer() {
        Ok(login) => Line::ok("gh", format!("logged in as {login}")),
        Err(ForgeError::Spawn(why)) => Line::missing(
            "gh",
            format!("cannot run `gh`: {why}"),
            "install the GitHub CLI (https://cli.github.com) and put `gh` on your PATH",
        ),
        Err(ForgeError::Failed(said)) => Line::missing(
            "gh",
            format!(
                "not logged in, or GitHub cannot be reached: {}",
                first_line(&said)
            ),
            "run `gh auth login`",
        ),
        Err(e) => Line::missing(
            "gh",
            e.to_string(),
            "update `gh`, then run `gh auth status`",
        ),
    }
}

/// The sandbox runtime in `tools`, with what it needs, which every agent runs in
pub(super) fn sandbox(host: &dyn Host, tools: &Tools) -> Line {
    let gaps = host.sandbox_gaps();
    if !gaps.is_empty() {
        let list = gaps.join(" and ");
        return Line::missing(
            "sandbox",
            format!(
                "the sandbox runtime needs {list}, which this machine lacks, so no agent can run"
            ),
            format!(
                "install {list} with your package manager, such as `sudo apt-get install bubblewrap socat` on Debian"
            ),
        );
    }
    let srt = tools.sandbox();
    if !srt.is_file() {
        return Line::missing(
            "sandbox",
            format!(
                "the sandbox runtime is not at {}, so no runner starts",
                srt.display()
            ),
            "run `shep kelpie tools install`",
        );
    }
    // `tools install` makes the copy that cuts the keychain out of a call's
    // profile, and a covered repo's calls run under nothing else.
    if host.has_keychain() && !tools.keychain_blocked() {
        return Line::missing(
            "sandbox",
            format!(
                "the sandbox runtime has no copy at {} that keeps a call from the keychain, \
                 so no call on a repo kelpie's App covers starts",
                tools.sandbox_without_keychain().display()
            ),
            "run `shep kelpie tools install`",
        );
    }
    Line::ok("sandbox", "the sandbox runtime can run")
}

/// What the sandbox cannot do on this machine, when it is Linux
///
/// srt skips the globs of `denyWrite` there, and turns every Unix socket off
/// whatever the policy lists (`docs/facts/worker-profile-and-sandbox.md`).
pub(super) fn sandbox_limits(host: &dyn Host) -> Option<Line> {
    (host.os() == "linux").then(|| {
        Line::ok(
            "sandbox: limits",
            "on Linux the sandbox fences only the nested harness folders that exist when a call \
             starts, so nested harness folders made during a call are caught after it, and \
             workers cannot reach the lease door, so `cargo-test` runs unleased",
        )
    })
}

/// The login a project's `claude` calls run with, for a repo the App covers (`covered`)
/// on a machine with a keychain, where the calls cannot reach the one `claude` keeps there
///
/// The runner reads the token from its sheep's `env`, whose secret-backed names are
/// `env`, and doctor from this shell's. A plain value there could be empty, which
/// the runner refuses and shep's reply does not show, so it does not count.
pub(super) fn login(
    subject: String,
    covered: bool,
    env: &[String],
    host: &dyn Host,
) -> Option<Line> {
    if !covered || !host.has_keychain() {
        return None;
    }
    let runner = env.iter().any(|name| name == LOGIN_VAR);
    Some(match (runner, host.login_token()) {
        (true, _) => Line::ok(
            subject,
            "claude logs in with CLAUDE_CODE_OAUTH_TOKEN from the runner's `env`, as the \
             keychain is blocked in its calls",
        ),
        (false, true) => Line::ok(
            subject,
            "claude logs in with CLAUDE_CODE_OAUTH_TOKEN, as the keychain is blocked in its calls",
        ),
        (false, false) => Line::missing(
            subject,
            "the keychain is blocked in this project's calls and CLAUDE_CODE_OAUTH_TOKEN is set \
             in neither the runner's `env` nor this shell, so claude has no login there",
            LOGIN_FIX,
        ),
    })
}

/// The shepherd, on the pinned shep line
pub(super) fn shepherd(client: &dyn Connection, shep_home: &Path) -> Line {
    let version = &client.daemon().daemon_version;
    Line::ok(
        "shepherd",
        format!("shep {version} at {}", shep_home.display()),
    )
}

/// kelpie's dog, which holds the leases and is `online` only once it has named itself
pub(super) fn dog(rows: &[ProcessInfo]) -> Line {
    match crate::flock::dog_problem(rows) {
        None => Line::ok("dog", "kelpie's dog is running and has named itself"),
        Some((what, fix)) => Line::missing("dog", what, fix),
    }
}

/// The shepherd kelpie could not use, and why
pub(super) fn refused(refused: &ConnectRefused, shep_home: &Path) -> Line {
    let fix = match refused {
        ConnectRefused::Unreachable(_) => {
            "start your shepherd, or set SHEP_HOME to the one you run"
        }
        ConnectRefused::Skew(_) => {
            "run a shep on kelpie's shep minor, at the version kelpie is built for or later"
        }
    };
    Line::missing("shepherd", refused.describe(shep_home), fix)
}
