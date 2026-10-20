//! A finding the worker showed was wrong

use serde::{Deserialize, Serialize};

use crate::ports::Finding;

/// A finding a fix turn disputed instead of fixing, with the worker's reason
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dispute {
    /// The finding as the findings file held it
    pub finding: Finding,
    /// Why the worker says it is wrong, naming its evidence
    pub reason: String,
    /// The reviewer that raised it
    pub raised_by: String,
    /// The round whose fix turn disputed it
    pub round: u32,
    /// Whether a reviewer's prompt has shown it since
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shown: bool,
}
