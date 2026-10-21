//! A lease the dog granted a project

use serde::{Deserialize, Serialize};

use crate::ports::Timestamp;
use crate::ticket::TicketId;

/// A lease the dog granted this project
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseHeld {
    /// What the lease is for
    pub resource: Resource,
    /// The ticket of the work item it was taken for
    #[serde(default)]
    pub issue: Option<TicketId>,
    /// When it was granted
    pub since: Timestamp,
}

/// A shared resource kelpie leases out
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resource {
    /// The GPU local review rounds run on
    Gpu,
    /// CodeRabbit's review window
    Coderabbit,
    /// cubic's review window
    Cubic,
    /// Codex's review allowance
    Codex,
}
