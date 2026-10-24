//! What kelpie tells the maintainer after it happens

use serde::{Deserialize, Serialize};

use crate::ticket::TicketId;

/// Something kelpie tells the maintainer after it happens
///
/// Not a ruling: it has no id and takes no answer. It goes once the
/// webhook post lands. The two kinds have no field in common, so a file
/// from before the second reads as the first.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Notice {
    /// A merge kelpie made under `auto`
    Merged(MergeNotice),
    /// A workflow on `main` that stayed red after a rerun, or went green again
    Main(MainNotice),
}

/// A merge kelpie made under `auto`
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeNotice {
    /// The work item's ticket
    pub issue: TicketId,
    /// The pull request merged
    pub pull_request: u64,
    /// The head it merged at
    pub head: String,
}

/// A workflow on `main` that is red for good or green again
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MainNotice {
    /// The workflow's name
    pub workflow: String,
    /// The run it names: the red one that stayed red, or the green one
    pub run: u64,
    /// How long the workflow was red when this was written, in seconds
    pub red_for: u64,
    /// The end of the red run's failed log, or none where it could not be
    /// read, and none for a green notice
    pub log: String,
    /// Whether the workflow went green
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub green: bool,
}
