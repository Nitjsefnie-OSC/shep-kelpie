//! A sub-issue, as its parent's close check reads it

/// How a sub-issue stands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubIssueState {
    /// Still open
    Open,
    /// Closed as completed, or with no reason the forge recorded
    Completed,
    /// Closed as not planned or as a duplicate, so nothing was built for it
    NotPlanned,
}

/// A sub-issue of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubIssue {
    /// Its number
    pub number: u64,
    /// How it stands
    pub state: SubIssueState,
}
