//! The pull requests from other people that kelpie follows

use serde::{Deserialize, Serialize};

use super::RulingKind;
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
    /// What the contribution's priced model calls have cost, over every head
    ///
    /// A call whose harness reports no dollars adds nothing. Left out while
    /// nothing is spent.
    #[serde(default, skip_serializing_if = "Cost::is_free")]
    pub spend: Cost,
    /// How many over-budget rulings were answered, each of which doubled the
    /// cap. Left out while none was.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub budget_raises: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
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
            spend: Cost::default(),
            budget_raises: 0,
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
