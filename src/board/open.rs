//! An open pull request as the board's one listing reads it, with what the
//! rework, gate and merge steps read of it

use crate::ports::{Checks, PullRequest, PullRequestState};

/// An open pull request, as the forge lists it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPullRequest {
    /// Its number
    pub number: u64,
    /// The branch it merges from
    pub head: String,
    /// The issues on the same repo that it closes when it merges
    pub closes: Vec<u64>,
    /// Its labels' names
    pub labels: Vec<String>,
    /// Whether it is still a draft
    pub draft: bool,
    /// Its head commit's hash
    pub commit: String,
    /// Where CI stands on its head commit
    pub checks: Checks,
    /// Whether its branch is on a fork rather than the repo itself
    pub from_fork: bool,
    /// Its author's login, empty for a deleted account
    pub author: String,
    /// How many reviews and threads it had when listed
    pub marks: ReviewMarks,
}

/// How many reviews and review threads a pull request has, for telling
/// whether any arrived between two listings
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewMarks {
    /// Reviews, whoever left them
    pub reviews: u64,
    /// Review threads, resolved or not
    pub threads: u64,
}

impl OpenPullRequest {
    /// The pull request as a gate or a merge reads it
    pub fn view(&self) -> PullRequest {
        PullRequest {
            state: PullRequestState::Open,
            draft: self.draft,
            head: self.commit.clone(),
            checks: self.checks.clone(),
            labels: self.labels.clone(),
        }
    }
}
