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

use crate::panel::{Ballot, Skip, Verdict, Voter};
use crate::ports::Timestamp;
use crate::runner::Answer;
use crate::settings::RulingName;
use crate::state::{Ruling, RulingKind, Stuck};
use crate::ticket::TicketId;

/// How many of the latest answered rulings the researcher is shown
const LAST: usize = 60;

/// The most of a ruling's question kept, in characters
const QUESTION: usize = 1_200;

/// Who answered a ruling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runner) enum Answerer {
    /// The maintainer, in its thread, at the terminal or on ntfy
    Maintainer,
    /// The project manager, for the maintainer
    Pm,
    /// The panel, by its votes
    Panel,
}

/// A ruling and how it was answered
// wire format: changing this is a breaking change to the rulings file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::runner) struct Record {
    pub(in crate::runner) id: TicketId,
    pub(in crate::runner) at: Timestamp,
    // When it was raised, if the state file said
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::runner) raised: Option<Timestamp>,
    pub(in crate::runner) issue: Option<TicketId>,
    pull_request: Option<u64>,
    asked: String,
    pub(in crate::runner) answer: String,
    // What a worker's question says as it asked it, which the project manager's
    // rewording of the card leaves as it is, or why a turn failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stem: Option<String>,
    // Whether the project manager answered for the maintainer
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    by_pm: bool,
    // Whether the panel decided it
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) by_panel: bool,
    // The kind of ruling, as `rulings.pm_decides` names it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::runner) kind: Option<RulingName>,
    // The option the answer is, when it is one of the ruling's
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) chose: Option<char>,
    // What the panel concluded, and each expert's ballot, once it voted
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::runner) ballots: Vec<Ballot>,
    // Why the panel cast no vote on it, if it listed two options or more and none was cast
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) skipped: Option<Skip>,
    // Who abstained since the answer came before their vote, which an
    // expert's failed call does not say
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) overtaken: Vec<Voter>,
}

impl Record {
    /// `ruling`, answered with `answer` at `at` by `by`
    pub(in crate::runner) fn of(
        ruling: &Ruling,
        answer: &Answer,
        at: Timestamp,
        by: Answerer,
    ) -> Self {
        let chose = crate::runner::voice::chosen(&ruling.kind, answer);
        let answer = match answer {
            Answer::Yes => "yes".to_owned(),
            Answer::No(note) => format!("no: {note}"),
            Answer::Rework(note) => format!("rework: {note}"),
            Answer::Text(text) => text.clone(),
            Answer::Trust(head) => format!("yes {head}"),
            Answer::Do(action) => action.word().to_owned(),
        };
        Self {
            id: ruling.id,
            at,
            raised: ruling.raised,
            issue: ruling.issue,
            pull_request: ruling.pull_request,
            asked: asked_of(ruling),
            stem: stem_of(ruling),
            answer: crate::runner::panel::folders::clip(&answer, QUESTION),
            by_pm: by == Answerer::Pm,
            by_panel: by == Answerer::Panel,
            kind: Some(RulingName::of(&ruling.kind)),
            chose,
            verdict: ruling.verdict.clone(),
            ballots: ruling.panel.clone().unwrap_or_default(),
            skipped: ruling.skipped,
            overtaken: Vec::new(),
        }
    }

    /// Whether the panel decided the same question as `ruling` asks, for the
    /// same work item: its pull request's number names a ruling once there is
    /// one, so the ticket is what stays the same
    pub(super) fn decided_by_panel(&self, ruling: &Ruling) -> bool {
        let item = match (self.issue, ruling.issue) {
            (Some(was), Some(is)) => was == is,
            _ => self.id == ruling.id,
        };
        self.by_panel && item && self.stem.is_some() && self.stem == stem_of(ruling)
    }

    /// Who answered it
    pub(in crate::runner) fn by(&self) -> Answerer {
        match (self.by_pm, self.by_panel) {
            (true, _) => Answerer::Pm,
            (_, true) => Answerer::Panel,
            _ => Answerer::Maintainer,
        }
    }

    /// Whether the maintainer answered it
    pub(super) fn by_maintainer(&self) -> bool {
        !self.by_pm && !self.by_panel
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

// What a worker asked in `ruling`, as `rulings.jsonl` keeps it.
// A failed turn's stem is why it failed, so the same failure again on the
// same work item is one the panel already decided.
fn stem_of(ruling: &Ruling) -> Option<String> {
    let stem = match &ruling.kind {
        RulingKind::Stuck(Stuck::TurnFailed { why, .. }) => {
            format!(
                "turn-failed: {}",
                why.split_whitespace().collect::<Vec<_>>().join(" ")
            )
        }
        kind => crate::runner::voice::question_stem(kind)?,
    };
    Some(crate::runner::panel::folders::clip(&stem, QUESTION))
}

// What `ruling` asks, less the table of options, as `rulings.jsonl` keeps it.
fn asked_of(ruling: &Ruling) -> String {
    let (brief, _) = crate::runner::voice::brief(ruling);
    let asked = brief.split("\n\n| | Option").next().unwrap_or_default();
    crate::runner::panel::folders::clip(asked.trim_start_matches('#').trim(), QUESTION)
}

/// Sets `fields` on ruling `id`'s line in the file at `file`, leaving every
/// other line and field as it is; a file with no such line is left alone
///
/// # Errors
///
/// What went wrong, for the log.
pub(in crate::runner) fn amend(
    file: &Path,
    id: TicketId,
    fields: Vec<(&str, serde_json::Value)>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read {}: {}", file.display(), e.kind()))?;
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let at = lines.iter().rposition(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok();
        value.is_some_and(|v| {
            serde_json::from_value::<TicketId>(v["id"].clone()).is_ok_and(|read| read == id)
        })
    });
    let Some(at) = at else {
        return Ok(());
    };
    let mut value: serde_json::Value =
        serde_json::from_str(&lines[at]).map_err(|e| e.to_string())?;
    for (name, field) in fields {
        value[name] = field;
    }
    lines[at] = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let kept = file.with_extension("jsonl.tmp");
    let wrote =
        std::fs::write(&kept, lines.join("\n") + "\n").and_then(|()| std::fs::rename(&kept, file));
    wrote.map_err(|e| format!("cannot rewrite {}: {}", file.display(), e.kind()))
}

/// Every ruling answered in the file at `file`, oldest first; a line that
/// does not read is skipped
pub(in crate::runner) fn records(file: &Path) -> Vec<Record> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    (text.lines())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
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
        about.extend(record.issue.map(|t| t.named().to_lowercase()));
        about.extend(record.pull_request.map(|n| format!("pull request #{n}")));
        let about = match about.is_empty() {
            true => String::new(),
            false => format!(" ({})", about.join(", ")),
        };
        let by = match (record.by_pm, record.by_panel) {
            (true, _) => "Answered by the project manager, not the maintainer",
            (_, true) => "Decided by the panel, not the maintainer",
            _ => "Answered by the maintainer",
        };
        out.push_str(&format!(
            "## Ruling {}{about}, on {}\n\n{}\n\n{by}: {}\n\n",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::ticket;

    // A ruling id from before a ruling was named by its ticket, where the
    // counter named no ticket at all (0), is an id no record can carry: the
    // line is skipped as any line that does not read is.
    #[test]
    fn a_line_whose_id_names_no_ticket_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rulings.jsonl");
        std::fs::write(
            &file,
            "{\"id\":0,\"at\":100,\"issue\":7,\"pull_request\":null,\"asked\":\"q\",\"answer\":\"yes\"}\n",
        )
        .unwrap();
        assert_eq!(records(&file), Vec::new());
        assert!(read(&file).contains("(none kept yet)"));
    }

    // An answered ruling of a kind that has since gone stays in the history.
    #[test]
    fn an_answered_ruling_of_a_removed_kind_is_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rulings.jsonl");
        let text = serde_json::json!({
            "id": 3, "at": 100, "issue": 7, "pull_request": null,
            "asked": "q", "answer": "yes", "kind": "local-model-spilled",
        });
        std::fs::write(&file, format!("{text}\n")).unwrap();
        let records = records(&file);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, ticket(3));
        assert!(read(&file).contains("yes"));
    }

    fn asking(id: u64, pull_request: Option<u64>, asked: &str) -> Ruling {
        let kind = crate::state::RulingKind::Question {
            asked: asked.into(),
            resume: crate::state::Resume::Nothing,
        };
        Ruling {
            issue: Some(ticket(7)),
            pull_request,
            ..Ruling::new(ticket(id), asked, kind)
        }
    }

    fn decided(ruling: &Ruling) -> Record {
        Record::of(
            ruling,
            &Answer::Text("A".into()),
            Timestamp(1),
            Answerer::Panel,
        )
    }

    // The ruling's id is its pull request's number once there is one, so a
    // question asked again after the pull request opened is the same question.
    #[test]
    fn a_question_asked_again_under_a_pull_request_is_the_one_the_panel_decided() {
        let before = decided(&asking(7, None, "Which flag?\n- `--a`\n- `--b`"));

        assert!(before.decided_by_panel(&asking(71, Some(71), "Which flag?\n- `--a`\n- `--b`")));
        assert!(!before.decided_by_panel(&asking(71, Some(71), "Which command?")));
    }

    fn failing(id: u64, why: &str) -> Ruling {
        let kind = RulingKind::from(Stuck::TurnFailed {
            why: why.into(),
            phase: crate::work_item::Phase::Implement,
            retry: crate::work_item::Turn::Due,
            restore: Vec::new(),
            setup: false,
        });
        Ruling {
            issue: Some(ticket(7)),
            pull_request: Some(71),
            ..Ruling::new(ticket(id), why, kind)
        }
    }

    // The same failure on the same work item, whitespace aside, is the one the
    // panel decided, and another failure is not.
    #[test]
    fn a_turn_that_fails_again_the_same_way_is_the_failure_the_panel_decided() {
        let before = decided(&failing(71, "harness  exited\nwith 1"));

        assert!(before.decided_by_panel(&failing(72, "harness exited with 1")));
        assert!(!before.decided_by_panel(&failing(72, "harness exited with 2")));
    }
}
