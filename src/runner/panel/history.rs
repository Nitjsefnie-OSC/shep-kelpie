//! The rulings the maintainer answered, kept for the researcher to read
//!
//! One JSON object a line in `<project>/rulings.jsonl`, appended as an
//! answer is saved. The state file keeps only the rulings that wait, so
//! without this the researcher could not find how an earlier one went.
//! A file that cannot be read or written costs the researcher its history
//! and nothing else.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;
use crate::runner::Answer;
use crate::state::Ruling;

/// How many of the latest answered rulings the researcher is shown
const LAST: usize = 60;

/// The most of a ruling's question kept, in characters
const QUESTION: usize = 1_200;

/// A ruling and how it was answered
// wire format: changing this is a breaking change to the rulings file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::runner) struct Record {
    id: u64,
    at: Timestamp,
    issue: Option<u64>,
    pull_request: Option<u64>,
    asked: String,
    answer: String,
}

impl Record {
    /// `ruling`, answered with `answer` at `at`
    pub(in crate::runner) fn of(ruling: &Ruling, answer: &Answer, at: Timestamp) -> Self {
        let (brief, _) = crate::runner::voice::brief(ruling);
        // What it asks, less the table of options.
        let asked = brief.split("\n\n| | Option").next().unwrap_or_default();
        let asked = asked.trim_start_matches('#').trim();
        let answer = match answer {
            Answer::Yes => "yes".to_owned(),
            Answer::No(note) => format!("no: {note}"),
            Answer::Rework(note) => format!("rework: {note}"),
            Answer::Text(text) => text.clone(),
        };
        Self {
            id: ruling.id,
            at,
            issue: ruling.issue,
            pull_request: ruling.pull_request,
            asked: crate::runner::panel::folders::clip(asked, QUESTION),
            answer: crate::runner::panel::folders::clip(&answer, QUESTION),
        }
    }

    /// Adds the record to the file at `file`
    ///
    /// # Errors
    ///
    /// What went wrong, for the log.
    pub(in crate::runner) fn append(&self, file: &Path) -> Result<(), String> {
        let line = serde_json::to_string(self).map_err(|e| e.to_string())?;
        let opened = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .and_then(|mut f| writeln!(f, "{line}"));
        opened.map_err(|e| format!("cannot add to {}: {}", file.display(), e.kind()))
    }
}

/// The latest answered rulings in the file at `file`, as text, oldest first
///
/// Nothing, as text, for a file that is not there; a line that does not read
/// is skipped.
pub(super) fn read(file: &Path) -> String {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let records: Vec<Record> = (text.lines())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let start = records.len().saturating_sub(LAST);
    let mut out = String::from("# Earlier rulings and how they were answered\n\n");
    if records.is_empty() {
        out.push_str("(none kept yet)\n");
    }
    for record in &records[start..] {
        let mut about = Vec::new();
        about.extend(record.issue.map(|n| format!("issue #{n}")));
        about.extend(record.pull_request.map(|n| format!("pull request #{n}")));
        let about = match about.is_empty() {
            true => String::new(),
            false => format!(" ({})", about.join(", ")),
        };
        out.push_str(&format!(
            "## Ruling {}{about}, on {}\n\n{}\n\nAnswered: {}\n\n",
            record.id,
            day(record.at),
            record.asked,
            record.answer
        ));
    }
    out
}

// The day `at` falls on, in UTC.
fn day(at: Timestamp) -> String {
    i64::try_from(at.0)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map_or_else(|| at.0.to_string(), |t| t.strftime("%Y-%m-%d").to_string())
}
