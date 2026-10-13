//! A CI run on a branch, as the forge lists it

use super::Timestamp;

/// The latest completed run of one workflow on a branch
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRun {
    /// The workflow's name
    pub workflow: String,
    /// The forge's id for the run
    pub id: u64,
    /// The commit it ran on
    pub head: String,
    /// What started it, such as `push` or `pull_request`
    pub event: String,
    /// How it ended, as the forge words it: `success`, `failure`, `cancelled`
    /// and the rest. What counts as red is the caller's to say.
    pub conclusion: String,
    /// When the run was created
    pub created: Timestamp,
}
