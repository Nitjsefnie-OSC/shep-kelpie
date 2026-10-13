//! The pull requests from other people that kelpie follows

use serde::{Deserialize, Serialize};

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
    /// The last head that passed the security review
    ///
    /// Left out until one has. The next head is read as the change since it,
    /// unless it is not an ancestor of the next head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<String>,
    /// What the contributor said and did on the pull request, oldest first,
    /// the last [`KEPT`]
    ///
    /// Ids, kinds and times only: the text is read again when a prompt needs
    /// it. Left out until there is something.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heard: Vec<Said>,
    /// What the maintainer wrote on the pull request, kept apart as context,
    /// the last [`KEPT`]
    ///
    /// Only comments and replies: the maintainer's pushes are no one's to
    /// hear. Left out until there is something.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<Said>,
}

/// How many of what was said a contribution keeps, in each list
pub const KEPT: usize = 50;

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
            heard: Vec::new(),
            context: Vec::new(),
        }
    }

    /// This contribution after a push to `head`, seen at `seen`: the head is
    /// unread, the last head that passed stays, and the push is heard
    #[must_use]
    pub fn pushed(&self, head: String, seen: Timestamp) -> Self {
        let mut next = Self {
            head: head.clone(),
            seen,
            security: None,
            ..self.clone()
        };
        next.hear(Said::Push { head, at: seen });
        next
    }

    /// Whether `said` is on record already, among what was heard or the context
    pub fn knows(&self, said: &Said) -> bool {
        let same = |known: &Said| known.same_as(said);
        self.heard.iter().any(same) || self.context.iter().any(same)
    }

    /// Records what the contributor said, keeping the last [`KEPT`]
    pub fn hear(&mut self, said: Said) {
        keep(&mut self.heard, said);
    }

    /// Records what the maintainer said, keeping the last [`KEPT`]
    pub fn overhear(&mut self, said: Said) {
        keep(&mut self.context, said);
    }

    /// When the latest thing on record happened: the place a read of the
    /// forge's comments starts from after a restart
    pub fn latest(&self) -> Timestamp {
        let times = self.heard.iter().chain(&self.context).map(Said::at);
        times.chain([self.seen]).max().unwrap_or(self.seen)
    }

    /// Whether the security review has read the current head
    pub fn is_read(&self) -> bool {
        self.security.as_ref().is_some_and(|s| s.head == self.head)
    }

    /// Whether the current head passed the security review: it is the last
    /// one that did
    pub fn is_passed(&self) -> bool {
        self.passed.as_deref() == Some(self.head.as_str())
    }
}

fn keep(list: &mut Vec<Said>, said: Said) {
    list.push(said);
    let over = list.len().saturating_sub(KEPT);
    list.drain(..over);
}

/// One thing said or done on a pull request, with no text
///
/// A comment and a review reply are numbered apart by the forge, so an id
/// means a thing only with its kind.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Said {
    /// A comment on the pull request's conversation
    Comment {
        /// The forge's id for it
        id: u64,
        /// When it was written
        at: Timestamp,
    },
    /// A comment in a review thread on its diff
    Reply {
        /// The forge's id for it
        id: u64,
        /// When it was written
        at: Timestamp,
    },
    /// A push: the head moved to this commit
    Push {
        /// The new head commit's hash
        head: String,
        /// When the board saw the head move, which is after the push
        at: Timestamp,
    },
}

impl Said {
    /// When it happened
    pub fn at(&self) -> Timestamp {
        match self {
            Self::Comment { at, .. } | Self::Reply { at, .. } | Self::Push { at, .. } => *at,
        }
    }

    // Whether both are the same thing: the same kind and id, whenever each
    // was seen
    fn same_as(&self, other: &Said) -> bool {
        match (self, other) {
            (Self::Comment { id: a, .. }, Self::Comment { id: b, .. })
            | (Self::Reply { id: a, .. }, Self::Reply { id: b, .. }) => a == b,
            (Self::Push { head: a, .. }, Self::Push { head: b, .. }) => a == b,
            _ => false,
        }
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
