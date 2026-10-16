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
///
/// A record that closed an issue as a duplicate is kept past the count: the
/// issue carries no `needs-triage` label, and the record is what keeps a
/// reopened one out of the due list.
pub const TRIAGED_KEPT: usize = 200;

impl TriageState {
    /// Drops the oldest finished records past [`TRIAGED_KEPT`], keeping every
    /// one that closed an issue as a duplicate
    pub fn prune(&mut self) {
        let kept = |t: &Triaged| {
            matches!(t.step, TriageStep::Done | TriageStep::Ready) && t.duplicate_of.is_none()
        };
        let mut extra = self
            .issues
            .iter()
            .filter(|t| kept(t))
            .count()
            .saturating_sub(TRIAGED_KEPT);
        self.issues.retain(|t| {
            let drop = extra > 0 && kept(t);
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
    /// The labels the research asked for that are not all on yet, so that a
    /// restart at `commented` puts them on
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The open issue or pull request this one was closed as a duplicate of,
    /// which the comment named before the close
    ///
    /// Left out unless the issue was closed as a duplicate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<u64>,
    /// The agent-ready body kelpie is owed to write over the issue's, kept so
    /// a restart writes it and never writes it twice
    ///
    /// Left out unless the issue is on the ready path and the edit is owed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_body: Option<String>,
    /// The labels the issue carried when this pass began, kept so a pass that
    /// resumes after a restart still tells a triage label the maintainer put on
    /// from kelpie's own
    ///
    /// Left out while empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<String>,
}

/// How far an issue's triage has got, saved before the next write to the forge
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriageStep {
    /// The comment is posted and the labels are not all on. On the ready path
    /// the `ready_body` edit is owed too
    Commented,
    /// The comment naming the other item is posted, and the close as a
    /// duplicate of `duplicate_of` is not
    Closed,
    /// The agent-ready body is written over the issue's and the type and
    /// priority labels are not all on
    BodyWritten,
    /// The labels the reply gave are on and `needs-triage` is not
    Labelled,
    /// `needs-triage` is on, or the maintainer's own triage label stood
    Done,
    /// `ready-for-agent` is on
    Ready,
}
