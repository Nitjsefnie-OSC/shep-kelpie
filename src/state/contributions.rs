//! The pull requests from other people that kelpie follows

use serde::{Deserialize, Serialize};

use super::{ProjectState, RulingKind};
use crate::ports::{Cost, Finding, Timestamp};
use crate::settings::PerItemUsd;

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
    /// Where the next read of the pull request's review threads starts
    ///
    /// The forge's comments are read from one place for the whole repo, but a
    /// review thread's only with its pull request, and a read of it can fail
    /// alone. Left out until one succeeds, when it is [`Self::seen`] at the
    /// first head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviews_from: Option<Timestamp>,
    /// What the contribution's priced model calls have cost, over every head
    ///
    /// A call whose harness reports no dollars adds nothing. Left out while
    /// nothing is spent.
    #[serde(default, skip_serializing_if = "Cost::is_free")]
    pub spend: Cost,
    /// How many times the cap was doubled: once for each answered over-budget
    /// ruling, and again while the spend was still above it. Left out while
    /// none was.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub budget_raises: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// What the calls on a pull request that is not followed any more cost, kept
/// for when it is followed again
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spent {
    /// The pull request
    pub pull_request: u64,
    /// What its calls cost, as [`Contribution::spend`]
    #[serde(default, skip_serializing_if = "Cost::is_free")]
    pub spend: Cost,
    /// How many times its cap was doubled, as [`Contribution::budget_raises`]
    #[serde(default, skip_serializing_if = "is_zero")]
    pub budget_raises: u32,
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
            spend: Cost::default(),
            budget_raises: 0,
            heard: Vec::new(),
            context: Vec::new(),
            reviews_from: None,
        }
    }

    /// The most its priced calls may cost before no further call starts:
    /// `per_item`, doubled once for each over-budget ruling answered
    pub fn budget_cap(&self, per_item: PerItemUsd) -> Cost {
        let doubled = 2u64.saturating_pow(self.budget_raises);
        Cost(per_item.cost().0.saturating_mul(doubled))
    }

    /// The cap its calls are over, or `None` while they cost no more than it
    pub fn over_budget(&self, per_item: PerItemUsd) -> Option<Cost> {
        let cap = self.budget_cap(per_item);
        (self.spend > cap).then_some(cap)
    }

    /// Lifts its cap for an answer to an over-budget ruling: one doubling, and
    /// more while its calls still cost more than the cap, so one answer
    /// always lets the next call start
    pub fn raise_budget(&mut self, per_item: PerItemUsd) {
        self.budget_raises = self.budget_raises.saturating_add(1);
        while self.over_budget(per_item).is_some() {
            self.budget_raises = self.budget_raises.saturating_add(1);
        }
    }

    /// This contribution with what the pull request spent while it was not
    /// followed, if it spent any
    #[must_use]
    pub fn with_spent(self, spent: Option<&Spent>) -> Self {
        Self {
            spend: spent.map_or(self.spend, |s| s.spend),
            budget_raises: spent.map_or(self.budget_raises, |s| s.budget_raises),
            ..self
        }
    }

    /// Where the next read of the review threads starts
    pub fn reviews_since(&self) -> Timestamp {
        self.reviews_from.unwrap_or(self.seen)
    }

    /// This contribution after a push to `head`, seen at `seen`: the head is
    /// unread, the last head that passed stays, and the push is heard
    #[must_use]
    pub fn pushed(&self, head: String, seen: Timestamp) -> Self {
        let mut next = Self {
            head: head.clone(),
            seen,
            security: None,
            // `seen` moves on, and the threads are read from where they were
            reviews_from: Some(self.reviews_since()),
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

    /// Records what the contributor said, keeping the last [`KEPT`], unless it
    /// was kept once and has dropped off the list since. Returns whether it
    /// was recorded.
    pub fn hear(&mut self, said: Said) -> bool {
        keep(&mut self.heard, said)
    }

    /// Records what the maintainer said, keeping the last [`KEPT`], on the
    /// same terms as [`Self::hear`]
    pub fn overhear(&mut self, said: Said) -> bool {
        keep(&mut self.context, said)
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

    /// Whether the current head passed the security review, or the maintainer
    /// trusted it: it is the last one that did
    pub fn is_passed(&self) -> bool {
        self.passed.as_deref() == Some(self.head.as_str())
    }
}

// A full list has let its oldest go, so a comment written before all of what
// it still holds was kept once and is not recorded again: it would only push
// out something newer. Pushes are not comments and are always new.
fn keep(list: &mut Vec<Said>, said: Said) -> bool {
    let dropped = list.len() >= KEPT
        && !matches!(said, Said::Push { .. })
        && list
            .iter()
            .map(Said::at)
            .min()
            .is_some_and(|oldest| said.at() < oldest);
    if dropped {
        return false;
    }
    list.push(said);
    let over = list.len().saturating_sub(KEPT);
    list.drain(..over);
    true
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
    /// The contribution's priced calls cost more than its cap, so no further
    /// call starts on it. A yes doubles the cap, again while it is below the
    /// spend, and so does a no with a note.
    OverBudget {
        /// The cap it was over: `budget.per_item_usd`, doubled once for each
        /// ruling like this one that was answered
        cap: Cost,
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

impl ProjectState {
    /// Follows `followed` from now on, keeping what the calls on the pull
    /// requests it drops cost, and giving up what the ones it takes back had
    pub fn follow(&mut self, followed: Vec<Contribution>) {
        let is_followed = |n: u64| followed.iter().any(|c| c.pull_request == n);
        let dropped: Vec<Spent> = (self.contributions.iter())
            .filter(|c| {
                !is_followed(c.pull_request) && !(c.spend.is_free() && c.budget_raises == 0)
            })
            .map(|c| Spent {
                pull_request: c.pull_request,
                spend: c.spend,
                budget_raises: c.budget_raises,
            })
            .collect();
        self.spent.retain(|s| {
            !is_followed(s.pull_request)
                && !dropped.iter().any(|d| d.pull_request == s.pull_request)
        });
        self.spent.extend(dropped);
        let surplus = self.spent.len().saturating_sub(KEPT);
        self.spent.drain(..surplus);
        self.contributions = followed;
    }

    /// What the pull request `n` spent while it was not followed
    pub fn spent_by(&self, n: u64) -> Option<&Spent> {
        self.spent.iter().find(|s| s.pull_request == n)
    }
}
