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
}
