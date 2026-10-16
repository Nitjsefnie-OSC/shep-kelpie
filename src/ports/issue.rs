//! An issue as the forge holds it, the open ones the follow-up check reads, and
//! a parent's sub-issues

use super::Timestamp;

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

/// An open issue whose sub-issues are all closed, as the forge counts them
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishedParent {
    /// Its number
    pub number: u64,
    /// How many sub-issues it has, all closed
    pub sub_issues: u64,
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

/// Why an issue was closed, which the forge records alongside the close
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueCloseReason {
    /// The work it asked for was done
    Completed,
    /// It will not be done
    NotPlanned,
    /// Another issue or pull request already covers it
    Duplicate,
}

impl IssueCloseReason {
    /// The name the GitHub API records it under
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not_planned",
            Self::Duplicate => "duplicate",
        }
    }
}

/// An open pull request, as the listing that tells a duplicate by reads it.
/// The board's own listing (`crate::board::OpenPullRequest`) is the other one,
/// with heads, checks and labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPull {
    /// Its number
    pub number: u64,
    /// Its title
    pub title: String,
    /// The issues its body names with `Resolves`, `Fixes` or `Closes`
    pub closes: Vec<u64>,
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
    /// The login of whoever opened it, empty when their account is gone
    pub author: String,
    /// Whether a bot opened it, the App included
    pub author_is_bot: bool,
    /// How the forge counts whoever opened it to the repo, or `None` when the
    /// listing does not say
    pub author_association: Option<AuthorAssociation>,
    /// Its labels' names
    pub labels: Vec<String>,
    /// When it was opened
    pub created: Timestamp,
}

/// How the forge counts the person who opened an issue to the repo it is on,
/// which is what it says per issue rather than per repo
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorAssociation {
    /// No history on the repo at all
    None,
    /// Opened their first issue or pull request on the repo
    FirstTimer,
    /// Wrote their first contribution to the repo
    FirstTimeContributor,
    /// Has contributed to the repo before
    Contributor,
    /// Belongs to the organisation that owns the repo
    Member,
    /// Has write access to the repo
    Collaborator,
    /// Owns the repo
    Owner,
}

impl AuthorAssociation {
    /// The association from the word the forge names it with, or `None` when
    /// the word is one kelpie does not know
    pub fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "NONE" => Some(Self::None),
            "FIRST_TIMER" => Some(Self::FirstTimer),
            "FIRST_TIME_CONTRIBUTOR" => Some(Self::FirstTimeContributor),
            "CONTRIBUTOR" => Some(Self::Contributor),
            "MEMBER" => Some(Self::Member),
            "COLLABORATOR" => Some(Self::Collaborator),
            "OWNER" => Some(Self::Owner),
            _ => None,
        }
    }

    /// Whether the forge counts this author as having no history on the repo:
    /// nothing of theirs is on it yet
    pub fn no_history(&self) -> bool {
        matches!(
            self,
            Self::None | Self::FirstTimer | Self::FirstTimeContributor
        )
    }
}
