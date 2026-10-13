//! Getting a build to install: a release's download, or a build of a git ref
//!
//! Both leave the binary in a folder under kelpie's home and hand back its
//! path. Nothing here touches the installed build.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process_group};

/// The repo a release or a ref comes from, unless `KELPIE_SOURCE` names another
pub const REPO: &str = "https://github.com/shep-pm/shep-kelpie.git";

/// The repo's slug, which `gh` takes
const SLUG: &str = "shep-pm/shep-kelpie";

/// How long each command of a ref build may run before it is ended
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// For each `git` command: clone, fetch and checkout
    pub git: Duration,
    /// For the `cargo build`
    pub cargo: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            git: NETWORK,
            cargo: Duration::from_secs(45 * 60),
        }
    }
}

/// How long a command that talks to the network gets
const NETWORK: Duration = Duration::from_secs(5 * 60);

// Runs `command` to its end with its output going to ours, so a build's
// progress shows, and says which command failed. A command still running at
// `limit` is ended with everything it started, which its own process group holds.
fn run(command: &mut Command, limit: Duration) -> Result<(), String> {
    let what = format!("{command:?}");
    let mut child = command
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot run {what}: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limit => {
                let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
                match group {
                    Some(group) => {
                        let _ = kill_process_group(group, Signal::KILL);
                    }
                    None => {
                        let _ = child.kill();
                    }
                }
                let _ = child.wait();
                return Err(format!("{what} did not end in {limit:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(format!("cannot wait for {what}: {e}")),
        }
    };
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{what} exited {status}"))
}

// A command's trimmed standard output, or none when it ran and refused.
fn ask(command: &mut Command) -> Result<Option<String>, String> {
    let what = format!("{command:?}");
    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {what}: {e}"))?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

/// Builds kelpie at `reference` of the repo at `repo`, in a clone kept under `work`
///
/// The clone stays between upgrades, so its `target` folder does too and a
/// later build compiles only what changed. `reference` is a branch, a tag or
/// a commit. Each command gets its limit from `limits`.
///
/// # Errors
///
/// A message naming the git or cargo command that failed or outlasted its limit.
pub fn build_ref(
    work: &Path,
    repo: &str,
    reference: &str,
    limits: Limits,
) -> Result<PathBuf, String> {
    if reference.is_empty() || reference.starts_with('-') {
        return Err(format!("{reference:?} is not a git ref"));
    }
    let clone = work.join("source");
    std::fs::create_dir_all(work).map_err(|e| format!("cannot make {}: {e}", work.display()))?;
    let git = |args: &[&str]| {
        let mut git = crate::spawn::command("git");
        git.arg("-C").arg(&clone).args(args);
        git
    };
    if clone.join(".git").exists() {
        run(
            &mut git(&["fetch", "--quiet", "--tags", "--force", "origin"]),
            limits.git,
        )?;
    } else {
        run(
            crate::spawn::command("git")
                .args(["clone", "--quiet", "--", repo])
                .arg(&clone),
            limits.git,
        )?;
    }
    // A branch is the remote's, not the stale local one a clone left behind.
    let commit = [format!("origin/{reference}"), reference.to_owned()]
        .iter()
        .find_map(|name| {
            let spec = format!("{name}^{{commit}}");
            ask(&mut git(&["rev-parse", "--verify", "--quiet", &spec]))
                .ok()
                .flatten()
        })
        .ok_or_else(|| format!("{repo} has no ref {reference}"))?;
    run(
        &mut git(&["checkout", "--quiet", "--force", "--detach", &commit]),
        limits.git,
    )?;
    // Its own target folder, wherever the maintainer's `CARGO_TARGET_DIR` points.
    let target = clone.join("target");
    run(
        crate::spawn::command("cargo")
            .args(["build", "--release", "--locked"])
            .env("CARGO_TARGET_DIR", &target)
            .current_dir(&clone),
        limits.cargo,
    )?;
    let binary = target.join("release/shep-kelpie");
    binary
        .is_file()
        .then_some(binary)
        .ok_or_else(|| format!("the build of {reference} made no target/release/shep-kelpie"))
}

/// The release asset that holds a build for this machine
pub fn asset() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        _ => "unknown-linux-gnu",
    };
    format!("shep-kelpie-{}-{os}.tar.gz", std::env::consts::ARCH)
}

/// Downloads release `version` of kelpie into a folder under `work`
///
/// The release is the tag `v<version>`, and its asset for this machine is a
/// gzipped tar holding `shep-kelpie`.
///
/// # Errors
///
/// A message when `version` is not a version, or `gh` or `tar` fails.
pub fn download_release(work: &Path, version: &str) -> Result<PathBuf, String> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+');
    if !version.starts_with(|c: char| c.is_ascii_digit()) || !version.chars().all(plain) {
        return Err(format!("{version:?} is not a release version"));
    }
    let folder = work.join(format!("release-{version}"));
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
    let asset = asset();
    run(
        crate::spawn::command("gh")
            .args(["release", "download", &format!("v{version}")])
            .args(["--repo", SLUG, "--pattern", &asset, "--clobber", "--dir"])
            .arg(&folder),
        NETWORK,
    )?;
    run(
        crate::spawn::command("tar")
            .arg("-xzf")
            .arg(folder.join(&asset))
            .arg("-C")
            .arg(&folder),
        NETWORK,
    )?;
    let binary = folder.join("shep-kelpie");
    binary
        .is_file()
        .then_some(binary)
        .ok_or_else(|| format!("{asset} held no shep-kelpie"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ref_that_looks_like_an_option_is_refused() {
        let work = tempfile::tempdir().unwrap();
        for reference in ["", "--upload-pack=x", "-b"] {
            let err = build_ref(work.path(), "/nowhere", reference, Limits::default()).unwrap_err();
            assert!(err.contains("is not a git ref"), "{err}");
        }
    }

    #[test]
    fn a_release_is_named_by_a_plain_version() {
        let work = tempfile::tempdir().unwrap();
        for version in ["", "../0.3.0", "0.3.0 --clobber", "latest"] {
            let err = download_release(work.path(), version).unwrap_err();
            assert!(err.contains("is not a release version"), "{err}");
        }
    }

    #[test]
    fn a_command_that_ends_in_time_returns_its_exit_status() {
        run(&mut Command::new("true"), Duration::from_secs(60)).unwrap();
        let err = run(&mut Command::new("false"), Duration::from_secs(60)).unwrap_err();
        assert!(err.contains("false") && err.contains("exited"), "{err}");
    }

    #[test]
    fn a_command_that_outlasts_its_limit_is_ended_with_its_children() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let started = Instant::now();
        let err = run(
            Command::new("sh")
                .args(["-c", "sleep 300 & echo $! > \"$0\"; wait"])
                .arg(&pid_file),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(30), "{err}");
        assert!(err.contains("sleep 300") && err.contains("300ms"), "{err}");
        let pid: u32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The orphan is reaped by init a moment after the kill.
        let gone = (0..200).any(|_| {
            let gone = !crate::upgrade::lock::alive(pid);
            if !gone {
                std::thread::sleep(Duration::from_millis(25));
            }
            gone
        });
        assert!(gone, "pid {pid} outlived its command");
    }

    #[test]
    fn the_asset_names_this_machine() {
        let asset = asset();
        assert!(asset.starts_with("shep-kelpie-"), "{asset}");
        assert!(asset.contains(std::env::consts::ARCH), "{asset}");
        assert!(asset.ends_with(".tar.gz"), "{asset}");
    }
}
