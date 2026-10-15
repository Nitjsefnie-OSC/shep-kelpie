//! What kelpie keeps of triaging issues other people open

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;

/// Where triaging stands
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageState {
    /// When `triage.pick` went past `off`: only an issue opened after it is
    /// triaged
    pub since: Timestamp,
    /// The issues triaged or being triaged, oldest first, the last [`TRIAGED_KEPT`]
    /// finished ones
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Triaged>,
}

/// How many finished records are kept, the oldest going first. An issue whose
/// record went is told apart from an untouched one by its `needs-triage` label
/// and, if that is taken off, by the marker in its comments.
pub const TRIAGED_KEPT: usize = 200;

impl TriageState {
    /// Drops the oldest records at `done` past [`TRIAGED_KEPT`]
    pub fn prune(&mut self) {
        let done = self.issues.iter().filter(|t| t.step == TriageStep::Done);
        let mut extra = done.count().saturating_sub(TRIAGED_KEPT);
        self.issues.retain(|t| {
            let drop = extra > 0 && t.step == TriageStep::Done;
            extra -= usize::from(drop);
            !drop
        });
    }
}

/// An issue whose triage comment kelpie has posted
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Triaged {
    /// The issue's number
    pub issue: u64,
    /// The id of the comment kelpie posted on it
    pub comment: u64,
    /// How far the issue has got
    pub step: TriageStep,
}

/// How far an issue's triage has got, saved before the next write to the forge
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriageStep {
    /// The comment is posted and the labels are not all on
    Commented,
    /// The labels the reply gave are on and `needs-triage` is not
    Labelled,
    /// `needs-triage` is on
    Done,
}
