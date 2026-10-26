//! The binding the ten baselines shipped with, and the step that writes it into the stubs
//!
//! A baseline once carried its `harness`, `model`, `effort` and `second_look`,
//! so a stub with only `extends: kelpie:<name>` inherited them. The baselines
//! now bind nothing, and [`migrate`] writes into each such stub the binding it
//! used to inherit, so it loads as it did.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::{AgentsError, create_temp, extends, failed};
use crate::settings::AgentName;

/// The binding one baseline shipped with
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The baseline's name
    pub name: &'static str,
    /// Its `harness`
    pub harness: &'static str,
    /// Its `model`
    pub model: &'static str,
    /// Its `effort`
    pub effort: &'static str,
    /// Whether it set `second_look: true`
    pub second_look: bool,
}

const fn row(
    name: &'static str,
    model: &'static str,
    effort: &'static str,
    second_look: bool,
) -> Row {
    Row {
        name,
        harness: "claude-code",
        model,
        effort,
        second_look,
    }
}

/// Every baseline's binding, as its file shipped before the baselines dropped their models
pub const TABLE: [Row; 10] = [
    row("defect-hunter", "claude-opus-5-5", "high", true),
    row("security-reviewer", "claude-opus-5-5", "high", false),
    row("spec", "claude-opus-5-5", "high", false),
    row("standards", "claude-opus-5-5", "high", false),
    row("issue-writer", "claude-opus-5-5", "medium", false),
    row("pm", "claude-opus-5-5", "medium", false),
    row("advisor", "claude-opus-5-5", "high", false),
    row("researcher", "claude-sonnet-5-5", "high", false),
    row("representative", "claude-opus-5-5", "low", false),
    row("triager", "claude-opus-5-5", "medium", false),
];

/// The binding the baseline `name` shipped with, if kelpie shipped one
pub fn named(name: &str) -> Option<&'static Row> {
    TABLE.iter().find(|row| row.name == name)
}

/// A file [`migrate`] wrote, or would
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrote {
    /// The file's name, such as `pm.md`
    pub file: String,
    // The keys added, with their values.
    added: Vec<(&'static str, &'static str)>,
}

impl Wrote {
    /// Whether the file set no `harness`, so it binds nothing until written
    pub fn was_unbound(&self) -> bool {
        self.added.iter().any(|(key, _)| *key == "harness")
    }
}

impl fmt::Display for Wrote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: wrote", self.file)?;
        for (key, value) in &self.added {
            write!(f, " {}", if *key == SECOND_LOOK { key } else { value })?;
        }
        Ok(())
    }
}

/// A file [`migrate`] left alone, because it needs a hand
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The file's name, such as `pm.md`
    pub file: String,
    /// What it sets and lacks
    pub why: String,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: skipped, {}", self.file, self.why)
    }
}

/// What [`migrate`] did to a folder, files in name order
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The files it wrote
    pub wrote: Vec<Wrote>,
    /// The files it left alone, which set another session `harness` without a `model` or `effort`
    pub skipped: Vec<Skipped>,
}

impl Report {
    /// A line for each file written, then one for each skipped
    pub fn lines(&self) -> Vec<String> {
        let wrote = self.wrote.iter().map(ToString::to_string);
        wrote
            .chain(self.skipped.iter().map(ToString::to_string))
            .collect()
    }
}

const SECOND_LOOK: &str = "second_look";

// A file to write: where, and its new text.
struct Planned {
    path: PathBuf,
    text: String,
}

/// What [`migrate`] would do to `folder`, writing nothing
///
/// # Errors
///
/// [`AgentsError::Read`] naming the folder or file that could not be read.
pub fn pending(folder: &Path) -> Result<Report, AgentsError> {
    plan(folder).map(|(report, _)| report)
}

/// Writes into each stub in `folder` the binding it inherited from its baseline
///
/// A file whose own text has `extends: kelpie:<baseline>` gets each of `harness`, `model`,
/// `effort` and (for `defect-hunter`) `second_look` it does not set, after its `extends` line;
/// comments, other keys and body stay. A file on another session `harness` than the baseline's
/// without both `model` and `effort` is skipped, and one on `command`, `endpoint` or `bot` is
/// left alone. A write renames a temporary file beside its target; a second run writes nothing.
///
/// # Errors
///
/// [`AgentsError::Read`] naming the folder or file that could not be read or
/// written.
pub fn migrate(folder: &Path) -> Result<Report, AgentsError> {
    let (report, planned) = plan(folder)?;
    for Planned { path, text } in planned {
        replace(&path, &text).map_err(|e| failed(&path, e))?;
    }
    Ok(report)
}

fn plan(folder: &Path) -> Result<(Report, Vec<Planned>), AgentsError> {
    let mut report = Report::default();
    let mut planned = Vec::new();
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((report, planned)),
        Err(e) => return Err(failed(folder, e)),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| failed(folder, e))?.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let named = AgentName::try_from(stem.to_owned()).is_ok();
        if named && path.extension().is_some_and(|e| e == "md") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| failed(&path, e))?;
        let file = path
            .file_name()
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        match judge(&text) {
            Judged::Leave => {}
            Judged::Skip(why) => report.skipped.push(Skipped { file, why }),
            Judged::Write { text, added } => {
                report.wrote.push(Wrote { file, added });
                planned.push(Planned { path, text });
            }
        }
    }
    Ok((report, planned))
}

enum Judged {
    Leave,
    Skip(String),
    Write {
        text: String,
        added: Vec<(&'static str, &'static str)>,
    },
}

// What to do with a file holding `text`.
fn judge(text: &str) -> Judged {
    let Some(row) = extends::shipped_parent(text).and_then(|parent| named(&parent)) else {
        return Judged::Leave;
    };
    let Some(own) = extends::own(text) else {
        return Judged::Leave;
    };
    let Some(after) = own.after_extends else {
        return Judged::Leave;
    };
    let sets = |key: &str| own.keys.iter().any(|k| k == key);
    let harness = own.harness.as_deref();
    // These take keys of their own, not the baseline's session keys, and never inherited them.
    if matches!(harness, Some("command" | "endpoint" | "bot")) {
        return Judged::Leave;
    }
    if harness.is_some_and(|h| h != row.harness) {
        let lacking: Vec<String> = ["model", "effort"]
            .into_iter()
            .filter(|k| !sets(k))
            .map(|k| format!("`{k}`"))
            .collect();
        if !lacking.is_empty() {
            return Judged::Skip(format!(
                "it sets `harness` but not {}, and `{}` ran on {}: set {} in the file",
                lacking.join(" or "),
                row.name,
                row.harness,
                lacking.join(" and "),
            ));
        }
    }
    let added: Vec<(&'static str, &'static str)> = [
        ("harness", row.harness),
        ("model", row.model),
        ("effort", row.effort),
        (SECOND_LOOK, "true"),
    ]
    .into_iter()
    .filter(|(key, _)| *key != SECOND_LOOK || row.second_look)
    .filter(|(key, _)| !sets(key))
    .collect();
    if added.is_empty() {
        return Judged::Leave;
    }
    let (head, rest) = text.split_at(after);
    let eol = if head.ends_with("\r\n") { "\r\n" } else { "\n" };
    let lines: String = added
        .iter()
        .map(|(key, value)| format!("{key}: {value}{eol}"))
        .collect();
    Judged::Write {
        text: format!("{head}{lines}{rest}"),
        added,
    }
}

// Writes `text` over `path`, or over what a symlink there points to, by a rename of a new file
// beside it, keeping its mode. A failure leaves no temporary file.
fn replace(path: &Path, text: &str) -> io::Result<()> {
    let path = &fs::canonicalize(path)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    let (beside, mut file) = create_temp(dir, &name)?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.set_permissions(fs::metadata(path)?.permissions()))
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&beside, path));
    if written.is_err() {
        // Best effort: the failure to report is the write's.
        let _ = fs::remove_file(&beside);
    }
    written
}
