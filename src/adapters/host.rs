//! This machine, as doctor reads it

use std::env;
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::adapters::{LOGIN_VAR, LoginToken};
use crate::doctor::host::{Host, gaps, sandbox_needs};

// How long `--version` has to answer, and how long output left in the pipe is
// waited for once the program has exited.
const VERSION_LIMIT: Duration = Duration::from_secs(10);
const DRAINED: Duration = Duration::from_secs(2);

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

    #[test]
    fn a_program_every_machine_has_is_on_the_path_and_a_made_up_one_is_not() {
        assert!(on_path("sh"));
        assert!(!on_path("kelpie-no-such-program"));
    }

    #[test]
    fn a_program_that_prints_a_version_is_read_and_one_that_hangs_or_is_missing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, body: &str| {
            let path = dir.path().join(name);
            crate::test::write_script(&path, &format!("#!/bin/sh\n{body}\n"));
            path
        };
        let limit = Duration::from_secs(5);

        let pi = script("pi", "echo 1.0.4\necho a second line");
        assert_eq!(version_within(&pi, limit), Ok("1.0.4".into()));

        let goose = script("goose", "echo ' 1.54.0'");
        assert_eq!(version_within(&goose, limit), Ok("1.54.0".into()));

        let hang = script("hang", "exec sleep 30");
        let started = Instant::now();
        let err = version_within(&hang, Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));

        let sick = script("sick", "echo 1.0.4\nexit 3");
        assert!(version_within(&sick, limit).unwrap_err().contains("exited"));

        let silent = script("silent", "exit 0");
        let err = version_within(&silent, limit).unwrap_err();
        assert!(err.contains("printed nothing"), "{err}");

        let err = version_within(dir.path().join("absent"), limit).unwrap_err();
        assert!(err.contains("cannot run"), "{err}");
    }
}
