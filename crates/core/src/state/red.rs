//! A workflow that is red on `main`

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;

/// A workflow whose latest completed run on `main` failed
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedWorkflow {
    /// The workflow's name
    pub workflow: String,
    /// The forge's id for the first red run kelpie read since it last read the
    /// workflow green, which is not the workflow's first red run when it failed
    /// before kelpie read it
    pub first_run: u64,
    /// The forge's id for its latest red run
    pub run: u64,
    /// The commit the latest red run ran on
    pub head: String,
    /// When `first_run` was created, so how long kelpie has known it red
    pub since: Timestamp,
    /// The run whose failed jobs kelpie reran when it first read the workflow
    /// red, once it has
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reran: Option<u64>,
    /// Whether the maintainer was told it stayed red, so a red stretch is
    /// told once
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub noticed: bool,
}
