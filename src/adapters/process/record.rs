//! A running agent call's record on disk
//!
//! It sits beside the call's output files and is written once the process
//! runs. It is rewritten when the call is asked to end, and removed when the
//! call ends, however it ends, so a record found later describes a call that
//! was still running.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ports::{AgentCall, Ending, Flying, Role};

/// The files one call keeps beside its settings path
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallFiles {
    /// The harness's stdout, which kelpie opens for it and reads when it ends
    pub stdout: PathBuf,
    /// The harness's stderr
    pub stderr: PathBuf,
    /// The call's record while it runs
    pub record: PathBuf,
}

impl CallFiles {
    pub(crate) fn of(call: &AgentCall) -> Self {
        Self {
            stdout: call.settings.with_extension("stdout.jsonl"),
            stderr: call.settings.with_extension("stderr.log"),
            record: call.settings.with_extension("call.json"),
        }
    }
}

/// What a running call's record says
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// The process the call runs as
    pub pid: u32,
    /// The process group it leads, which ends with whatever it spawned
    pub pgid: u32,
    /// The role the call is made for
    pub role: Role,
    /// The issue of the work item it is made for, and none for the project
    /// manager's call
    pub issue: Option<u64>,
    /// Where its stdout and stderr go
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    /// Whether the call was asked to end
    pub ending: bool,
    /// What the runner noted of the call, if it noted any
    pub flying: Option<Flying>,
}

/// A call's record to be kept for as long as its process runs
pub(super) struct Recorded {
    path: PathBuf,
    record: Record,
}

impl Recorded {
    /// The record of `call` running as `pid`, writing its output to `outputs`
    pub(super) fn new(
        call: &AgentCall,
        path: &Path,
        pid: u32,
        outputs: [&Path; 2],
        ending: &Ending,
    ) -> Self {
        let [stdout, stderr] = outputs.map(Path::to_path_buf);
        let record = Record {
            pid,
            pgid: pid,
            role: call.role,
            issue: (call.role != Role::Pm).then_some(call.issue),
            stdout,
            stderr,
            ending: ending.asked(),
            flying: ending.carried(),
        };
        Self {
            path: path.to_path_buf(),
            record,
        }
    }

    /// Writes the record as it stands
    ///
    /// A record that cannot be written is told and let go: the call runs
    /// without one.
    pub(super) fn write(&self) {
        let text = serde_json::to_string(&self.record).expect("a record is JSON");
        // Renamed into place, so a reader sees all of it or none.
        let temporary = self.path.with_extension("json.tmp");
        let written =
            std::fs::write(&temporary, text).and_then(|()| std::fs::rename(&temporary, &self.path));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temporary);
            eprintln!("cannot write {}: {e}", self.path.display());
        }
    }

    /// Writes the record again with the call asked to end
    pub(super) fn ending(&mut self) {
        if !self.record.ending {
            self.record.ending = true;
            self.write();
        }
    }
}

impl Drop for Recorded {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
