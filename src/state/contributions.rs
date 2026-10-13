//! The pull requests from other people that kelpie follows

use serde::{Deserialize, Serialize};

use super::RulingKind;
use crate::ports::{Finding, Timestamp};

/// A pull request from someone else, as the board last saw it
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    /// The pull request
    pub pull_request: u64,
    /// Its author's login
    pub author: String,
    /// Its head commit's hash
    pub head: String,
    /// When the board first saw that head
    pub seen: Timestamp,
    /// The head the security review last read, and what it found
    ///
    /// Left out until a head is read. A new head clears it, so it names the
    /// current `head` or nothing, and a head with none is not yet read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Security>,
    /// The last head that passed the security review, or that the maintainer
    /// trusted with a yes to its ruling
    ///
    /// Left out until one has. The next head is read as the change since it,
    /// unless it is not an ancestor of the next head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<String>,
}

impl Contribution {
    /// A contribution at `head`, first seen at `seen`, that no one has read
    pub fn new(pull_request: u64, author: String, head: String, seen: Timestamp) -> Self {
        Self {
            pull_request,
            author,
            head,
            seen,
            security: None,
            passed: None,
        }
    }

    /// Whether the security review has read the current head
    pub fn is_read(&self) -> bool {
        self.security.as_ref().is_some_and(|s| s.head == self.head)
    }

    /// Whether the current head passed the security review, or the maintainer
    /// trusted it: it is the last one that did
    pub fn is_passed(&self) -> bool {
        self.passed.as_deref() == Some(self.head.as_str())
    }
}

/// What the security review made of one head
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Security {
    /// The head it read
    pub head: String,
    /// What it found
    pub verdict: SecurityVerdict,
}

/// The verdict on a head
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityVerdict {
    /// Nothing in it needs a human's eye, so it passed
    Passed,
    /// Something in it does, each a finding with its file, line and why
    Flagged(Vec<Finding>),
}

/// Why a contribution waits on the maintainer, saved in its ruling's kind as
/// `reason`
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Held {
    /// The security review flagged the head. Nothing of it runs until the
    /// maintainer answers: a yes trusts it, and a no leaves it unpassed.
    Security {
        /// The head the lines are in
        head: String,
        /// Each file, line and why the review flagged
        lines: Vec<Finding>,
    },
}

impl RulingKind {
    /// Whether the ruling is about someone else's pull request. It is never
    /// posted on that pull request nor put to the panel: a public comment
    /// would accuse the contributor and tip off an attacker, and text from a
    /// stranger must not reach a vote.
    pub fn is_contribution(&self) -> bool {
        matches!(self, Self::Contribution(_))
    }
}
