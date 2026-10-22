//! This machine, as doctor reads it
//!
//! What doctor asks of the machine itself, beyond the runner's ports.

use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::adapters::{LOGIN_VAR, LoginToken};

// How long `--version` has to answer, and how long output left in the pipe is
// waited for once the program has exited.
const VERSION_LIMIT: Duration = Duration::from_secs(10);
const DRAINED: Duration = Duration::from_secs(2);

/// The machine kelpie runs on
pub trait Host: Send + Sync + fmt::Debug {
    /// The programs the sandbox runtime needs that are not on this machine,
    /// none when it can run
    fn sandbox_gaps(&self) -> Vec<&'static str>;

    /// Whether this machine keeps logins in a keychain a sandboxed command can ask
    fn has_keychain(&self) -> bool;

    /// This machine's operating system, as `std::env::consts::OS` names it
    fn os(&self) -> &'static str;

    /// Whether this shell's environment holds the login token `claude setup-token` makes
    fn login_token(&self) -> bool;

    /// Whether `program` is an executable file in a folder of `PATH`
    fn on_path(&self, program: &str) -> bool;

    /// The first line `program --version` prints, given ten seconds to print it
    ///
    /// # Errors
    ///
    /// A message when `program` does not run, does not answer in time, exits
    /// non-zero, or prints nothing.
    fn program_version(&self, program: &str) -> Result<String, String>;
}

/// Whether `found` is `minimum` or later, comparing major, minor and patch as numbers
///
/// None when either does not read as three dot-separated numbers, which a
/// pre-release such as `1.0.4-beta.1` does not.
pub fn at_least(found: &str, minimum: &str) -> Option<bool> {
    Some(numbers(found)? >= numbers(minimum)?)
}

fn numbers(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.');
    let mut number = || {
        let part = parts.next()?;
        // `parse` alone would take a leading `+`.
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let numbers = [number()?, number()?, number()?];
    parts.next().is_none().then_some(numbers)
}

/// The programs the sandbox runtime needs on `os`, as `std::env::consts::OS` names it
pub const fn sandbox_needs(os: &str) -> &'static [&'static str] {
    match os.as_bytes() {
        b"macos" => &["sandbox-exec"],
        b"linux" => &["bwrap", "socat"],
        _ => &[],
    }
}

/// The programs in `needs` that `on_path` does not find
pub fn gaps(needs: &[&'static str], on_path: impl Fn(&str) -> bool) -> Vec<&'static str> {
    needs
        .iter()
        .copied()
        .filter(|program| !on_path(program))
        .collect()
}

/// The machine kelpie is running on
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHost;

impl Host for SystemHost {
    fn sandbox_gaps(&self) -> Vec<&'static str> {
        gaps(sandbox_needs(env::consts::OS), on_path)
    }

    fn has_keychain(&self) -> bool {
        env::consts::OS == "macos"
    }

    fn os(&self) -> &'static str {
        env::consts::OS
    }

    fn login_token(&self) -> bool {
        env::var(LOGIN_VAR).is_ok_and(|token| LoginToken::new(token).is_some())
    }

    fn on_path(&self, program: &str) -> bool {
        on_path(program)
    }

    fn program_version(&self, program: &str) -> Result<String, String> {
        version_within(program, VERSION_LIMIT)
    }
}

// The first line `program --version` prints, with the child killed when it
// runs over `limit`.
fn version_within(program: impl AsRef<OsStr>, limit: Duration) -> Result<String, String> {
    let name = program.as_ref().to_string_lossy().into_owned();
    let mut child = crate::spawn::command(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {name}: {e}"))?;
    let mut pipe = child.stdout.take();
    let (send, heard) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe.as_mut() {
            let _ = pipe.read_to_end(&mut bytes);
        }
        let _ = send.send(bytes);
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{name} --version did not answer in {}s",
                    limit.as_secs_f32()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("cannot wait for {name}: {e}")),
        }
    };
    if !status.success() {
        return Err(format!("{name} --version exited {status}"));
    }
    let said = heard.recv_timeout(DRAINED).unwrap_or_default();
    String::from_utf8_lossy(&said)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} --version printed nothing"))
}

// A program is on the path when a folder of `PATH` holds an executable file
// of its name. macOS keeps `sandbox-exec` in `/usr/bin`, which `PATH` names.
fn on_path(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|folder| {
        std::fs::metadata(Path::new(&folder).join(program))
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::Fake;

    #[test]
    fn macos_needs_its_own_sandbox_and_linux_needs_two_programs() {
        assert_eq!(sandbox_needs("macos"), ["sandbox-exec"]);
        assert_eq!(sandbox_needs("linux"), ["bwrap", "socat"]);
        assert!(sandbox_needs("windows").is_empty());
    }

    #[test]
    fn a_version_is_compared_by_its_numbers_not_its_text() {
        for under in ["1.0.4", "0.99.2", "0.85.1", "1.0.3", "0.100.0"] {
            assert_eq!(at_least("1.0.10", under), Some(true), "1.0.10 over {under}");
            assert_eq!(
                at_least(under, "1.0.10"),
                Some(false),
                "{under} under 1.0.10"
            );
        }
        assert_eq!(at_least("1.0.4", "1.0.4"), Some(true));
        for text in [
            "1.0.4-beta.1",
            "1.0",
            "1.0.4.1",
            "v1.0.4",
            "",
            "latest",
            "1.0.x",
            "1.+0.4",
        ] {
            assert_eq!(at_least(text, "1.0.4"), None, "{text:?} found");
            assert_eq!(at_least("1.0.4", text), None, "{text:?} minimum");
        }
    }

    #[test]
    fn a_gap_is_a_needed_program_off_the_path() {
        let needs = sandbox_needs("linux");
        assert_eq!(gaps(needs, |_| true), Vec::<&str>::new());
        assert_eq!(gaps(needs, |p| p == "bwrap"), ["socat"]);
        assert_eq!(gaps(needs, |_| false), ["bwrap", "socat"]);
    }

    #[test]
    fn a_program_every_machine_has_is_on_the_path_and_a_made_up_one_is_not() {
        assert!(on_path(if cfg!(windows) { "cmd" } else { "sh" }));
        assert!(!on_path("kelpie-no-such-program"));
    }

    #[test]
    fn a_program_that_prints_a_version_is_read_and_one_that_hangs_or_is_missing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, fake: Fake| crate::test::write_fake(dir.path(), name, &fake);
        let limit = Duration::from_secs(5);

        let pi = script("pi", Fake::new().prints("1.0.4").prints("a second line"));
        assert_eq!(version_within(&pi, limit), Ok("1.0.4".into()));

        let goose = script("goose", Fake::new().prints(" 1.54.0"));
        assert_eq!(version_within(&goose, limit), Ok("1.54.0".into()));

        let hang = script("hang", Fake::new().sleeps(30));
        let started = Instant::now();
        let err = version_within(&hang, Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));

        let sick = script("sick", Fake::new().prints("1.0.4").exits(3));
        assert!(version_within(&sick, limit).unwrap_err().contains("exited"));

        let silent = script("silent", Fake::new());
        let err = version_within(&silent, limit).unwrap_err();
        assert!(err.contains("printed nothing"), "{err}");

        let err = version_within(dir.path().join("absent"), limit).unwrap_err();
        assert!(err.contains("cannot run"), "{err}");
    }
}
