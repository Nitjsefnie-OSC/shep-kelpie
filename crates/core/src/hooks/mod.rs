//! A project's own hooks, run by kelpie at its own points
//!
//! A project names commands in `hooks`, each for a [`HookPoint`]. Kelpie runs
//! them itself, so a pi or Codex worker, a reviewer, the issue writer and
//! kelpie's own posts reach them as a Claude Code worker does, with no
//! harness-shaped hook. The agent can neither edit nor skip a hook. One run
//! by `kelpie guard` or `kelpie confine` (`commit`, `push`, `command`,
//! `write`, `after_command`, a worker's `publish`) is inside kelpie's sandbox,
//! in the worker's fence, so it reads what the worker can; the points of kelpie's
//! own work item loop (`handover`, `turn_start`, `turn_end` and the notices)
//! and kelpie's own `publish` run in the runner, outside it.
//!
//! The command runs under `sh -c` with a JSON payload on its stdin: the
//! point, the project, the role whose text or work it judges, the repo's
//! slug, and the point's own fields. Its exit is its answer: 0 passes, 2
//! refuses with its stderr as the reason, and anything else is a hook that
//! failed. A failed hook refuses and says so, since a check that cannot run
//! must not pass silently. A hook that runs past [`TIMEOUT`] is killed and
//! has failed. A hook only refuses: what it prints on stdout is dropped, so
//! it cannot rewrite the text it judges. A point is a gate, which refuses, or
//! a notice (`reset`, `ruling`, `ci`, `done`), which [`Hooks::notify`] runs and
//! only logs when a hook exits non-zero or hangs.

use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::metrics::{Feature, Recorder};
use crate::settings::{HookPoint, ProjectHook};
use crate::worktree::WorktreeError;

mod shared;
pub use shared::SharedHooks;

/// How long a hook may run before it is killed and counts as failed
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How long one `kelpie guard` process may spend on all its hooks
///
/// Claude Code lets a call through when its guard hook outlasts the hook's
/// timeout, so the guard stops running hooks before that, and the hooks left
/// count as failed and refuse. The adapter sets the hook's timeout above this.
pub const GUARD_BUDGET: Duration = Duration::from_secs(120);

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
    /// When all hooks must be done, set for a guard process by [`Hooks::take`]
    #[serde(skip)]
    deadline: Option<Instant>,
    /// Where each run is counted, set in the runner by [`Hooks::counted_by`]
    #[serde(skip)]
    counted: Counted,
}

// The recorder a hook's runs go to, which no two hooks differ by.
#[derive(Debug, Clone, Default)]
struct Counted(Option<Arc<Recorder>>);

impl PartialEq for Counted {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Counted {}

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
            deadline: None,
            counted: Counted::default(),
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
            deadline: None,
            counted: Counted::default(),
        }
    }

    /// These hooks, each run of which `metrics` counts as `ok` or `failed`
    ///
    /// A hook that refuses on purpose ran as it should and is `ok`. Hooks run
    /// by `kelpie guard` or `kelpie confine` are in another process and are
    /// not counted.
    #[must_use]
    pub fn counted_by(self, metrics: Arc<Recorder>) -> Self {
        Self {
            counted: Counted(Some(metrics)),
            ..self
        }
    }

    /// These hooks, run in `folder` unless a call names another
    #[must_use]
    pub fn in_folder(self, folder: &Path) -> Self {
        Self {
            folder: Some(folder.to_owned()),
            ..self
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
        Ok((hooks.with_budget(GUARD_BUDGET), rest))
    }

    /// These hooks, which together may run for `budget` from now on: a hook
    /// that would run past it is stopped, and any left after it fail
    #[must_use]
    pub fn with_budget(self, budget: Duration) -> Self {
        Self {
            deadline: Some(Instant::now() + budget),
            ..self
        }
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
        let payload = self.payload(point, role, details);
        let folder = folder.or(self.folder.as_deref());
        for entry in self.list.iter().filter(|e| e.point == point) {
            self.run_entry(entry, &payload, folder)?;
        }
        Ok(())
    }

    /// Runs every hook for `point` as a notice: one that exits non-zero or
    /// hangs stops neither the rest nor the caller
    ///
    /// Returns a line for the log naming each hook that did not exit 0, or
    /// `None` when every one did.
    pub fn notify(
        &self,
        point: HookPoint,
        role: Role,
        folder: Option<&Path>,
        details: Value,
    ) -> Option<String> {
        let payload = self.payload(point, role, details);
        let folder = folder.or(self.folder.as_deref());
        let told: Vec<String> = (self.list.iter())
            .filter(|e| e.point == point)
            .filter_map(|entry| self.run_entry(entry, &payload, folder).err())
            .map(|refusal| {
                let Refusal {
                    program, reason, ..
                } = refusal;
                let point = point.as_str();
                format!("the `{point}` hook `{program}` did not exit 0 ({reason}); kelpie went on")
            })
            .collect();
        (!told.is_empty()).then(|| told.join("\n"))
    }

    // The JSON a hook reads: the point, project, role and repo, then the
    // point's own fields.
    fn payload(&self, point: HookPoint, role: Role, details: Value) -> String {
        let mut payload = json!({
            "point": point.as_str(),
            "project": self.project,
            "role": role.as_str(),
            "repo": self.repo,
        });
        if let (Some(payload), Value::Object(details)) = (payload.as_object_mut(), details) {
            payload.extend(details);
        }
        payload.to_string()
    }

    // Runs one hook on `payload`, within what is left of the budget.
    fn run_entry(
        &self,
        entry: &Entry,
        payload: &str,
        folder: Option<&Path>,
    ) -> Result<(), Refusal> {
        let ran = match self.allowed() {
            Some(timeout) => run(&entry.command, payload.as_bytes(), folder, timeout),
            None => Err((
                true,
                "the guard's time for hooks ran out before it could run".to_owned(),
            )),
        };
        if let Some(metrics) = &self.counted.0 {
            let failed = matches!(ran, Err((true, _)));
            metrics.feature_ran(Feature::Hooks, if failed { "failed" } else { "ok" });
        }
        ran.map_err(|(failed, reason)| Refusal {
            point: entry.point,
            program: program_of(&entry.command),
            failed,
            reason,
        })
    }

    // How long the next hook may run: its own timeout, or what is left of the
    // budget, whichever is less. None when nothing is left.
    fn allowed(&self) -> Option<Duration> {
        let Some(deadline) = self.deadline else {
            return Some(self.timeout);
        };
        let left = deadline.checked_duration_since(Instant::now())?;
        (!left.is_zero()).then_some(left.min(self.timeout))
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

// Plain patches: no colour, and no program the repo's config names.
const PLAIN: [&str; 4] = [
    "--unified=0",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
];

/// The fields of a `push` or `handover` payload for the commits `source`
/// holds past `origin`'s base branch, read with `run`, a trusted git of the
/// worktree: `None` when it holds none
///
/// # Errors
///
/// [`WorktreeError`] naming the git command that failed.
pub fn commits_past_base(
    run: &dyn Fn(&[&str]) -> Result<String, WorktreeError>,
    source: &str,
    base: &str,
) -> Result<Option<Value>, WorktreeError> {
    let tip = format!("refs/remotes/origin/{base}");
    let log = run(&["log", "--format=%H%x1f%B%x1e", source, "--not", &tip])?;
    let commits: Vec<Value> = log
        .split('\x1e')
        .filter_map(|entry| {
            let (sha, message) = entry.trim_start().split_once('\x1f')?;
            Some(json!({ "sha": sha, "message": message.trim_end() }))
        })
        .collect();
    if commits.is_empty() {
        return Ok(None);
    }
    let patch = ["log", "-p", "--format=", source, "--not", &tip];
    let diff = run(&[&patch[..], &PLAIN].concat())?;
    let head = run(&["rev-parse", source])?.trim().to_owned();
    let base_sha = run(&["rev-parse", &tip])?.trim().to_owned();
    Ok(Some(json!({
        "range": format!("{base_sha}..{head}"),
        "base": base_sha,
        "head": head,
        "commits": commits,
        "diff": diff,
    })))
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
