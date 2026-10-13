//! An issue as the forge holds it, the open ones the follow-up check reads, and
//! a parent's sub-issues

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

/// An issue as the forge holds it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Its title
    pub title: String,
    /// Its body, as written
    pub body: String,
    /// Its labels' names
    pub labels: Vec<String>,
    /// Whether it is still open
    pub open: bool,
    /// The issue it is a sub-issue of, if any
    pub parent: Option<u64>,
    /// The issues it is blocked by, as far as the forge lists them
    pub blocked_by: Vec<u64>,
}

/// An open issue, as the follow-up check reads it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenIssue {
    /// Its number
    pub number: u64,
    /// Its title
    pub title: String,
    /// Its body, as written
    pub body: String,
}
