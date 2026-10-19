//! What happened to the work items, kept for the batch retro
//!
//! One JSON object a line in `<project>/journal.jsonl`, appended as the
//! runner reports a step. The state file forgets a work item when it
//! finishes, and the step reports go to the sheep's log, so without this the
//! retro could not tell what the project manager's wakes, the reviews, the
//! CI fixes and the conflicts of a work item were. A file that cannot be
//! read or written costs the retro its digest's detail and nothing else.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ports::{Finding, Severity, Timestamp};
use crate::runner::StepReport;
use crate::ticket::TicketId;

/// One thing that happened
// wire format: changing this is a breaking change to the journal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "kebab-case")]
pub(in crate::runner) enum Entry {
    /// The project manager answered a wake
    Wake {
        woke_for: Vec<String>,
        acted: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        dropped: Vec<String>,
    },
    /// CI failed on a work item's pull request
    CiFailed {
        issue: TicketId,
        checks: Vec<String>,
    },
    /// CI failed on a work item's pull request with a log that named no file
    /// the branch changed, and these are the tests the log shows failing
    CiOutsideDiff { issue: TicketId, tests: Vec<String> },
    /// A work item's branch was rebased onto `main`
    Rebased { issue: TicketId },
    /// A work item's branch conflicted with `main`
    Conflicted { issue: TicketId, files: Vec<String> },
    /// A review round reported findings
    Round {
        issue: TicketId,
        round: u32,
        reviewer: String,
        findings: usize,
    },
    /// A round's findings went to the worker's fix turn
    FindingsSent {
        issue: TicketId,
        round: u32,
        held: usize,
    },
    /// The findings a round sent to the worker, as the worker was sent them
    Findings {
        issue: TicketId,
        round: u32,
        findings: Vec<Held>,
    },
    /// What slowed a work item's worker down, from its pull request's body
    Friction { issue: TicketId, text: String },
    /// A batch retro started over the work items finished until now
    Retro { issues: Vec<TicketId> },
}

/// A finding as the journal keeps it: where it is and what is wrong, no more
// wire format: changing this is a breaking change to the journal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::runner) struct Held {
    pub(super) severity: Severity,
    pub(super) file: String,
    pub(super) line: u32,
    pub(super) what: String,
}

/// The most characters of a finding's `what` the journal keeps
const WHAT_KEPT: usize = 160;

impl Held {
    /// What the journal keeps of `finding`
    pub(in crate::runner) fn of(finding: &Finding) -> Self {
        Self {
            severity: finding.severity,
            file: finding.file.clone(),
            line: finding.line,
            what: finding.what.chars().take(WHAT_KEPT).collect(),
        }
    }
}

impl Entry {
    /// What `report` adds to the journal, if anything
    pub(in crate::runner) fn of(report: &StepReport) -> Option<Self> {
        Some(match report {
            StepReport::PmAnswered {
                woke_for,
                acted,
                dropped,
                ..
            } => Self::Wake {
                woke_for: woke_for.clone(),
                acted: acted.clone(),
                dropped: dropped.clone(),
            },
            StepReport::CiFailed { issue, checks, .. } => Self::CiFailed {
                issue: *issue,
                checks: checks.clone(),
            },
            StepReport::Rebased { issue, .. } => Self::Rebased { issue: *issue },
            StepReport::Conflicted { issue, files, .. } => Self::Conflicted {
                issue: *issue,
                files: files.clone(),
            },
            StepReport::ReviewRound {
                issue,
                round,
                reviewer,
                findings,
                ..
            } => Self::Round {
                issue: *issue,
                round: *round,
                reviewer: reviewer.as_str().to_owned(),
                findings: *findings,
            },
            StepReport::BotReviewed {
                issue,
                round,
                reviewer,
                open_threads,
                ..
            } => Self::Round {
                issue: *issue,
                round: *round,
                reviewer: reviewer.as_str().to_owned(),
                findings: *open_threads,
            },
            StepReport::ReviewFindingsSent {
                issue, round, held, ..
            } => Self::FindingsSent {
                issue: *issue,
                round: *round,
                held: *held,
            },
            _ => return None,
        })
    }

    /// Whether the entry is about the work item for `ticket`
    ///
    /// A wake is about the work items it names by id.
    pub(super) fn names(&self, ticket: TicketId) -> bool {
        match self {
            Self::Wake {
                woke_for,
                acted,
                dropped,
                ..
            } => woke_for
                .iter()
                .chain(acted)
                .chain(dropped)
                .any(|text| mentions(text, ticket)),
            Self::CiFailed { issue: n, .. }
            | Self::Rebased { issue: n }
            | Self::Conflicted { issue: n, .. }
            | Self::CiOutsideDiff { issue: n, .. }
            | Self::Findings { issue: n, .. }
            | Self::Round { issue: n, .. }
            | Self::FindingsSent { issue: n, .. }
            | Self::Friction { issue: n, .. } => *n == ticket,
            Self::Retro { issues } => issues.contains(&ticket),
        }
    }
}

// Whether `text` names `ticket` as a whole id, and not the start of a longer one
fn mentions(text: &str, ticket: TicketId) -> bool {
    let tag = ticket.shown();
    text.match_indices(&tag).any(|(at, _)| {
        let after = text[at + tag.len()..].chars().next();
        !after.is_some_and(|c| c.is_ascii_digit())
    })
}

/// An entry and when it was made
// wire format: changing this is a breaking change to the journal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::runner) struct Stamped {
    pub(super) at: Timestamp,
    pub(super) entry: Entry,
}

/// Adds `entry`, made at `at`, to the journal at `file`
///
/// # Errors
///
/// What went wrong, for the log.
pub(in crate::runner) fn append(file: &Path, at: Timestamp, entry: Entry) -> Result<(), String> {
    let line = serde_json::to_string(&Stamped { at, entry }).map_err(|e| e.to_string())?;
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut f| writeln!(f, "{line}"));
    opened.map_err(|e| format!("cannot add to {}: {}", file.display(), e.kind()))
}

/// Every entry in the journal at `file`, oldest first; a line that does not
/// read is skipped
pub(in crate::runner) fn read(file: &Path) -> Vec<Stamped> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    (text.lines())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::ticket;

    fn wake(woke_for: &str) -> Entry {
        Entry::Wake {
            woke_for: vec![woke_for.to_owned()],
            acted: Vec::new(),
            dropped: Vec::new(),
        }
    }

    #[test]
    fn a_wake_names_a_work_item_only_by_its_whole_ticket_id() {
        let entry = wake("#12 conflicts with main");
        assert!(entry.names(ticket(12)));
        assert!(!entry.names(ticket(1)));
        assert!(!entry.names(ticket(123)));
        let entry = wake("k12 conflicts with main");
        assert!(entry.names("k12".parse().unwrap()));
        assert!(!entry.names("k123".parse().unwrap()));
    }

    #[test]
    fn entries_read_back_oldest_first_and_a_bad_line_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("journal.jsonl");
        let at = |n| Timestamp(n);
        append(&file, at(1), Entry::Rebased { issue: ticket(3) }).unwrap();
        std::fs::write(
            &file,
            format!("{}not json\n", std::fs::read_to_string(&file).unwrap()),
        )
        .unwrap();
        append(&file, at(2), wake("#3 stuck")).unwrap();

        let read = read(&file);
        assert_eq!(
            read,
            [
                Stamped {
                    at: at(1),
                    entry: Entry::Rebased { issue: ticket(3) }
                },
                Stamped {
                    at: at(2),
                    entry: wake("#3 stuck")
                },
            ]
        );
    }

    // The journal a kelpie that keyed its entries by issue number wrote.
    #[test]
    fn a_journal_line_that_kept_an_issue_number_reads_as_that_github_ticket() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("journal.jsonl");
        std::fs::write(
            &file,
            "{\"at\":1,\"entry\":{\"entry\":\"rebased\",\"issue\":3}}\n",
        )
        .unwrap();
        assert_eq!(
            read(&file),
            [Stamped {
                at: Timestamp(1),
                entry: Entry::Rebased { issue: ticket(3) }
            }]
        );
    }

    #[test]
    fn a_missing_journal_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&dir.path().join("none.jsonl")), []);
    }
}
