//! A project's own hooks, run by kelpie at its own points
//!
//! A project names commands in `hooks`, each for a [`HookPoint`]. Kelpie runs
//! them itself, so a pi or Codex worker, a reviewer, the issue writer and
//! kelpie's own posts reach them as a Claude Code worker does, with no
//! harness-shaped hook. A hook runs outside the agent's sandbox, so it can
//! read config the agent cannot, and the agent can neither edit nor skip it.
//!
//! The command runs under `sh -c` with a JSON payload on its stdin: the
//! point, the project, the role whose text or work it judges, the repo's
//! slug, and the point's own fields. Its exit is its answer: 0 passes, 2
//! refuses with its stderr as the reason, and anything else is a hook that
//! failed. A failed hook refuses and says so, since a check that cannot run
//! must not pass silently. A hook that runs past [`TIMEOUT`] is killed and
//! has failed. A hook only refuses: what it prints on stdout is dropped, so
//! it cannot rewrite the text it judges.

use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::settings::{HookPoint, ProjectHook};

/// How long a hook may run before it is killed and counts as failed
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How `kelpie guard`'s command line carries the project's hooks, as JSON
pub const FLAG: &str = "--hooks=";

/// The exit that refuses, with the hook's stderr as the reason
const REFUSES: i32 = 2;

/// The most of a hook's stderr kept as its reason, in characters
const REASON_LIMIT: usize = 1000;

/// Whose text or work a hook judges
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A worker: its commits, pushes, handover and pull request
    Worker,
    /// The issue writer, whose issues reach the forge as its own `gh` calls
    IssueWriter,
    /// Kelpie itself: rulings, replies on review threads, the issues it files
    Kelpie,
}

impl Role {
    /// The role as the payload spells it
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::IssueWriter => "issue-writer",
            Self::Kelpie => "kelpie",
        }
    }
}

/// The project's hooks, with what the payload names the project by
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hooks {
    project: String,
    repo: String,
    list: Vec<Entry>,
    /// Where a hook runs unless the call names a folder
    #[serde(skip)]
    folder: Option<PathBuf>,
    /// How long a hook may run
    #[serde(skip, default = "timeout")]
    timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    point: HookPoint,
    command: String,
}

fn timeout() -> Duration {
    TIMEOUT
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            project: String::new(),
            repo: String::new(),
            list: Vec::new(),
            folder: None,
            timeout: TIMEOUT,
        }
    }
}

impl Hooks {
    /// The hooks `hooks` names for `project`, whose repo is `repo`, run in
    /// `folder` unless a call names another
    pub fn new(project: &str, repo: &str, folder: &Path, hooks: &[ProjectHook]) -> Self {
        Self {
            project: project.to_owned(),
            repo: repo.to_owned(),
            list: hooks
                .iter()
                .map(|h| Entry {
                    point: h.point,
                    command: h.command.as_str().to_owned(),
                })
                .collect(),
            folder: Some(folder.to_owned()),
            timeout: TIMEOUT,
        }
    }

    /// These hooks, each allowed `timeout` instead of [`TIMEOUT`]
    #[must_use]
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Whether there is no hook at any point
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether any hook runs at `point`
    pub fn has(&self, point: HookPoint) -> bool {
        self.list.iter().any(|e| e.point == point)
    }

    /// The hooks as `kelpie guard`'s command line carries them, or none
    /// when there are none
    pub fn flag(&self) -> Option<String> {
        let json = serde_json::to_string(self).ok()?;
        (!self.is_empty()).then(|| format!("{FLAG}{json}"))
    }

    /// The hooks `args` carries, and the rest of `args`
    ///
    /// # Errors
    ///
    /// What to tell the worker, when the flag does not hold hooks.
    pub fn take(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let mut hooks = Self::default();
        let mut rest = Vec::new();
        for arg in args {
            match arg.strip_prefix(FLAG) {
                Some(json) => {
                    hooks = serde_json::from_str(json)
                        .map_err(|e| format!("kelpie guard cannot read its hooks: {e}"))?;
                }
                None => rest.push(arg.clone()),
            }
        }
        Ok((hooks, rest))
    }

    /// Runs every hook for `point`, stopping at the first that refuses
    ///
    /// `details` holds the point's own fields, merged into the payload. The
    /// hook runs in `folder`, or in the folder these hooks were made with.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] of the first hook that refused or failed.
    pub fn check(
        &self,
        point: HookPoint,
        role: Role,
        folder: Option<&Path>,
        details: Value,
    ) -> Result<(), Refusal> {
        let mut payload = json!({
            "point": point.as_str(),
            "project": self.project,
            "role": role.as_str(),
            "repo": self.repo,
        });
        if let (Some(payload), Value::Object(details)) = (payload.as_object_mut(), details) {
            payload.extend(details);
        }
        let payload = payload.to_string();
        let folder = folder.or(self.folder.as_deref());
        for entry in self.list.iter().filter(|e| e.point == point) {
            let ran = run(&entry.command, payload.as_bytes(), folder, self.timeout);
            if let Err((failed, reason)) = ran {
                return Err(Refusal {
                    point,
                    program: program_of(&entry.command),
                    failed,
                    reason,
                });
            }
        }
        Ok(())
    }

    /// Checks text bound for the forge: `field` names what it is, such as
    /// "the comment", and `text` is what would be posted
    ///
    /// # Errors
    ///
    /// The [`Refusal`] of the first publish hook that refused or failed.
    pub fn publish(&self, role: Role, field: &str, text: &str) -> Result<(), Refusal> {
        if !self.has(HookPoint::Publish) {
            return Ok(());
        }
        let details = json!({ "field": field, "text": text });
        self.check(HookPoint::Publish, role, None, details)
    }
}

/// A hook that refused, or that could not run and so refuses
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The point it ran at
    pub point: HookPoint,
    /// Its program's file name, which names it without a path of this machine's
    pub program: String,
    /// Whether it failed to run, as against refusing on purpose
    pub failed: bool,
    /// What it said, or why it failed
    pub reason: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            point,
            program,
            reason,
            ..
        } = self;
        let point = point.as_str();
        match self.failed {
            true => write!(
                f,
                "the `{point}` hook `{program}` failed, and a check that cannot run refuses: \
                 {reason}"
            ),
            false => write!(f, "the `{point}` hook `{program}` refused: {reason}"),
        }
    }
}

impl core::error::Error for Refusal {}

// The file name of the command's program, past any `NAME=value` words.
fn program_of(command: &str) -> String {
    let word = command
        .split_whitespace()
        .find(|w| !w.contains('=') || w.starts_with(['/', '~', '$']))
        .unwrap_or_default()
        .trim_matches(['\'', '"']);
    Path::new(word)
        .file_name()
        .map_or(word, |name| name.to_str().unwrap_or(word))
        .to_owned()
}

// Runs `command` on `payload`: `Err((failed, reason))` when it refuses.
fn run(
    command: &str,
    payload: &[u8],
    folder: Option<&Path>,
    timeout: Duration,
) -> Result<(), (bool, String)> {
    let failed = |why: String| Err((true, why));
    let mut process = Command::new("sh");
    process
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(folder) = folder.filter(|f| f.is_dir()) {
        process.current_dir(folder);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(e) => return failed(format!("it could not be started: {e}")),
    };
    // Written and read apart from the wait, so a payload or a stderr larger
    // than a pipe holds cannot stop the hook from ending.
    let input = child.stdin.take();
    let payload = payload.to_vec();
    std::thread::spawn(move || {
        if let Some(mut input) = input {
            // A hook that never reads its stdin closes it, which is no error.
            let _ = input.write_all(&payload);
        }
    });
    let (said, heard) = mpsc::channel();
    if let Some(mut stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.by_ref().take(1 << 20).read_to_end(&mut bytes);
            let _ = said.send(bytes);
        });
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return failed(format!(
                    "it ran past {}s and was stopped",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return failed(format!("it could not be waited on: {e}")),
        }
    };
    // A program the hook left running may hold stderr open: wait a moment.
    let stderr = heard
        .recv_timeout(Duration::from_secs(1))
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
        .unwrap_or_default();
    let said = |empty: &str| match stderr.is_empty() {
        true => empty.to_owned(),
        false => stderr.chars().take(REASON_LIMIT).collect(),
    };
    match status.code() {
        Some(0) => Ok(()),
        Some(REFUSES) => Err((false, said("it gave no reason"))),
        Some(code) => {
            let why = format!("it exited with {code}");
            Err((true, join(&why, &stderr)))
        }
        None => Err((true, join("it was stopped by a signal", &stderr))),
    }
}

// `why`, then what the hook printed, if anything.
fn join(why: &str, stderr: &str) -> String {
    match stderr.is_empty() {
        true => why.to_owned(),
        false => format!(
            "{why}: {}",
            stderr.chars().take(REASON_LIMIT).collect::<String>()
        ),
    }
}

#[cfg(test)]
mod tests;
