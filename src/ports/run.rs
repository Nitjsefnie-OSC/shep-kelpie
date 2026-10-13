//! The CI runs on a branch and the workflows of a repo, as the forge lists them

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

/// A workflow of a repo, as the forge lists it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    /// The workflow's name, which its runs carry as [`BranchRun::workflow`]
    pub name: String,
    /// Where its file is in the repo
    pub path: String,
    /// Whether the forge still runs it
    pub state: WorkflowState,
}

/// A workflow's state, as the forge words it
///
/// The values are the ones GitHub documents for a workflow's `state` field in
/// "List repository workflows".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    /// `active`
    Active,
    /// `deleted`: its file is gone from the repo
    Deleted,
    /// `disabled_manually`
    DisabledManually,
    /// `disabled_inactivity`
    DisabledInactivity,
    /// `disabled_fork`
    DisabledFork,
}
