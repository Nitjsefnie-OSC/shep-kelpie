//! The forge ledger: every call kelpie makes of GitHub
//!
//! `<kelpie home>/forge.jsonl` holds one JSON object per line, appended and
//! never rewritten, for every project's runner together: a line as each
//! forge call ends, held calls included. A line is one call through a port,
//! the [`Forge`](crate::ports::Forge) of the maintainer's login or the
//! [`Voice`](crate::ports::Voice) of kelpie's App. A port call can ask
//! GitHub more than once, so a line is a lower bound on requests. `shep
//! kelpie usage --forge` reads it, and nothing else does.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;

mod kind;
mod report;

pub use kind::{Api, Kind};
pub use report::report;

#[cfg(test)]
mod tests;

/// The ledger's file name in kelpie's home
pub const FILE: &str = "forge.jsonl";

/// How a call ended
// wire format: changing this is a breaking change to the forge ledger
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ended {
    /// GitHub answered it
    Ok,
    /// It failed for a reason other than a limit
    Failed,
    /// It was never made, because an earlier answer had the limit used up
    Held,
    /// GitHub answered that a rate limit is used up
    UsedUp,
}

/// One line of the ledger: a forge call, as it ended
// wire format: changing this is a breaking change to the forge ledger
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// When it ended
    pub at: Timestamp,
    /// The project whose runner made it
    pub project: String,
    /// What it was
    pub kind: Kind,
    /// Which API it spent from
    pub api: Api,
    /// How long it took, in milliseconds; none for a held call
    pub millis: u64,
    /// How it ended
    pub ended: Ended,
}

/// Where a runner appends its forge calls
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    project: String,
    // Whether the last append failed, so a run of failures is told once
    failing: AtomicBool,
}

impl Ledger {
    /// The ledger in kelpie's home `kelpie_home`, for `project`'s calls
    pub fn in_home(kelpie_home: &Path, project: &str) -> Self {
        Self {
            path: kelpie_home.join(FILE),
            project: project.to_owned(),
            failing: AtomicBool::new(false),
        }
    }

    /// Appends a call of `kind` that ended `ended` after `millis` at `at`. A
    /// failure is told on stderr, once until an append works again, and goes
    /// no further: no forge call fails on it.
    pub fn record(&self, at: Timestamp, kind: Kind, millis: u64, ended: Ended) {
        let line = Line {
            at,
            project: self.project.clone(),
            kind,
            api: kind.api(),
            millis,
            ended,
        };
        match append_to(&self.path, &line) {
            Ok(()) => self.failing.store(false, Ordering::Relaxed),
            Err(e) if !self.failing.swap(true, Ordering::Relaxed) => eprintln!(
                "cannot add to the forge ledger {}, so forge calls go unrecorded there \
                 until it can be written: {e}",
                self.path.display()
            ),
            Err(_) => {}
        }
    }
}

// The name the ledger gives a value
fn wire_name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

// The whole line goes in one `write`, never retried: on a file opened to
// append, that lands at the end whole, so runners appending together do not
// mix lines. A short write is an error and leaves a torn tail, which the
// next append ends with a newline of its own.
fn append_to(path: &Path, line: &Line) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(line).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut file = (OpenOptions::new().create(true).read(true).append(true)).open(path)?;
    let length = file.metadata()?.len();
    if length > 0 {
        let mut last = [0u8];
        file.seek(SeekFrom::Start(length - 1))?;
        file.read_exact(&mut last)?;
        if last != *b"\n" {
            bytes.insert(0, b'\n');
        }
    }
    if file.write(&bytes)? == bytes.len() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "the line was only partly written",
        ))
    }
}

/// Every line of the ledger at `path` that reads as one, oldest first: a
/// line torn by a crash, or of a kind a newer kelpie wrote, is passed over
///
/// # Errors
///
/// The [`io::Error`] reading the file failed with, apart from its absence,
/// which reads as no lines.
pub fn read(path: &Path) -> io::Result<Vec<Line>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
}
