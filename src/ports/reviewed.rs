//! A pull request as a rework or an adoption reads it

use super::PullRequestState;

/// A pull request as a rework or an adoption reads it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewed {
    /// Open, merged or closed
    pub state: PullRequestState,
    /// Its title
    pub title: String,
    /// Its body, as written
    pub body: String,
    /// The issues on the same repo that it closes when it merges
    pub closes: Vec<u64>,
    /// The branch it merges into
    pub base: String,
    /// The branch it merges from
    pub branch: String,
    /// Whether that branch is on a fork rather than the repo itself
    pub from_fork: bool,
    /// Its author's login, empty for a deleted account
    pub author: String,
    /// Whether it is still a draft
    pub draft: bool,
    /// Its labels' names
    pub labels: Vec<String>,
    /// The maintainer's latest review: the latest one a person left, since
    /// kelpie's own reviewers post none and bots are left out
    pub review: Option<MaintainerReview>,
}

/// One review the maintainer left, as written
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainerReview {
    /// The forge's id for it
    pub id: String,
    /// Whether it requests changes, which only a reviewer other than the
    /// pull request's author can do
    pub changes_requested: bool,
    /// Its body, empty when it has none
    pub body: String,
    /// Its comments whose threads are still unresolved, in the forge's order
    pub comments: Vec<ReviewComment>,
}

/// One comment on a line of a pull request's diff
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    /// The file it is on
    pub file: String,
    /// The line, when the forge still places it
    pub line: Option<u32>,
    /// What it says
    pub body: String,
}
