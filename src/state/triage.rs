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
    /// The issues triaged or being triaged, oldest first
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<Triaged>,
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
