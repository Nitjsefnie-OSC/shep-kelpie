//! The board: which ready issue becomes the next work item
//!
//! The board's input is the project's open issues labelled `ready-for-agent`.
//! The board keeps them in an order and dispatches the first that is free. A
//! newly ready issue joins it after the last issue of its priority: `priority: P0`,
//! then `P1`, `P2`, `P3`, `P4`, then an issue with no priority label, and within a
//! priority the oldest first, by ticket id. An issue whose priority label
//! changes moves the same way. An issue that closes or loses the label leaves
//! it. An issue that already
//! has an open pull request or an assignee is someone's work in progress, and
//! is skipped. So is one whose `agent:` label names no implementer the
//! project lists, since kelpie would not know which agent to run it on. An
//! issue waits while any issue it is blocked by is open, even one with a pull
//! request. An issue the runner picks but cannot take, because the forge
//! cannot show it or its `agent:` label fails when `add` reads it, is skipped
//! too, so it cannot stall the issues behind it. An issue with sub-issues is
//! never worked itself: its sub-issues are.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::ports::Timestamp;
use crate::runner::IN_PROGRESS;
use crate::settings::AgentName;
use crate::ticket::TicketId;

pub mod briefing;
mod open;

pub use open::{OpenPullRequest, ReviewMarks};

/// The label that puts an issue on the board
pub const READY: &str = "ready-for-agent";

/// The label of an issue nobody has read yet, which stays off the board
pub const TRIAGE: &str = "needs-triage";

/// The prefix of the label that ranks an issue on the board
pub const PRIORITY_LABEL: &str = "priority: ";

/// The prefix of the label that names the implementer an issue runs on
pub const AGENT_LABEL: &str = "agent:";

/// What ends an `agent:` label that pins its work item to the agent it names
pub const PIN: char = '!';

/// The prefix of the label that named the worker's model before `agent:`
const OLD_WORKER_LABEL: &str = "worker:";

// The priority labels, highest first. An issue with none ranks after them all.
const PRIORITIES: [&str; 5] = [
    "priority: P0",
    "priority: P1",
    "priority: P2",
    "priority: P3",
    "priority: P4",
];

/// An open issue labelled [`READY`], as its source lists it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyIssue {
    /// Its id
    pub ticket: TicketId,
    /// Its title
    pub title: String,
    /// Its body, as written
    pub body: String,
    /// Whether anyone is assigned to it
    pub assigned: bool,
    /// Its labels' names
    pub labels: Vec<String>,
    /// The issues it is blocked by, as far as the forge lists them
    pub blocked_by: Vec<Blocker>,
    /// How many more blockers the forge counted but did not list
    pub unlisted_blockers: u64,
    /// The ticket it is a sub-issue of, if any
    pub parent: Option<TicketId>,
    /// Its own sub-issues, as the forge counts them
    pub sub_issues: SubIssues,
}

/// How many sub-issues an issue has, and how many of them are closed
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubIssues {
    /// All of them
    pub total: u64,
    /// The closed ones
    pub closed: u64,
}

impl SubIssues {
    /// Whether it has sub-issues and every one is closed
    pub fn all_closed(self) -> bool {
        self.total > 0 && self.closed >= self.total
    }
}

/// A ticket a ready ticket is blocked by
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocker {
    /// Its id
    pub ticket: TicketId,
    /// Whether the forge shows it open
    pub open: bool,
}

/// Why the board passed over a ready issue
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum Skip {
    /// An open pull request closes it
    PullRequest {
        /// The ticket
        issue: TicketId,
        /// The pull request
        pull_request: u64,
    },
    /// Kelpie already finished a work item for it
    Finished {
        /// The ticket
        issue: TicketId,
    },
    /// `drop` ended its work item, and the forge has listed it as ready since
    Dropped {
        /// The ticket
        issue: TicketId,
    },
    /// Someone is assigned to it
    Assigned {
        /// The ticket
        issue: TicketId,
    },
    /// It carries the `in-progress` label, so an earlier worker holds it
    InProgress {
        /// The ticket
        issue: TicketId,
    },
    /// It has sub-issues, which are worked in its place
    Split {
        /// The ticket
        issue: TicketId,
        /// Its sub-issues still open
        open: u64,
    },
    /// A ticket it is blocked by is still open
    Blocked {
        /// The ticket
        issue: TicketId,
        /// Its open blockers, lowest first
        by: Vec<TicketId>,
        /// Blockers the forge did not list, each taken as open
        #[serde(skip_serializing_if = "is_zero")]
        unlisted: u64,
    },
    /// Its `agent:` label names no listed implementer
    Label {
        /// The ticket
        issue: TicketId,
        /// Why
        error: LabelError,
    },
    /// The forge could not show it when the runner came to take it
    Failed {
        /// The ticket
        issue: TicketId,
        /// Why, as the refusal reads
        error: String,
    },
    /// Its pull request asked for a rework that could not start this poll
    Rework {
        /// The ticket
        issue: TicketId,
        /// The pull request
        pull_request: u64,
        /// Why, as the refusal reads
        error: String,
    },
    /// Its body names a file the branch of a work item parked on a ruling
    /// changes, so it waits for that item to move
    Overlap {
        /// The ticket
        issue: TicketId,
        /// The parked work item's ticket
        with: TicketId,
        /// The files both touch
        files: Vec<String>,
    },
    /// It waits a pass for the board to read the paths its body names, or,
    /// with `unknown`, the files of that parked work item's branch, before
    /// the overlap with parked branches can be checked
    PathsUnread {
        /// The ticket
        issue: TicketId,
        /// The parked work item whose branch's files are not known yet
        #[serde(skip_serializing_if = "Option::is_none")]
        unknown: Option<TicketId>,
    },
    /// It has no `agent:` label, and waits for the issue writer to pick
    /// one, or, with `ruling`, for the maintainer to answer that ruling
    Unlabelled {
        /// The ticket
        issue: TicketId,
        /// The ruling it waits on, since the issue writer could not pick
        #[serde(skip_serializing_if = "Option::is_none")]
        ruling: Option<u64>,
    },
    /// An adopted pull request that could not start this poll, and still waits
    Adopt {
        /// The pull request
        pull_request: u64,
        /// Why, as the refusal reads
        error: String,
        /// When the pull request first failed to start, and has every
        /// pass since
        since: Timestamp,
    },
}

impl Skip {
    /// The ticket passed over, or `None` for an adopted pull request, which
    /// goes before every ticket
    pub fn issue(&self) -> Option<TicketId> {
        match self {
            Self::PullRequest { issue, .. }
            | Self::Finished { issue }
            | Self::Dropped { issue }
            | Self::Assigned { issue }
            | Self::InProgress { issue }
            | Self::Split { issue, .. }
            | Self::Blocked { issue, .. }
            | Self::Label { issue, .. }
            | Self::Failed { issue, .. }
            | Self::Rework { issue, .. }
            | Self::Overlap { issue, .. }
            | Self::PathsUnread { issue, .. }
            | Self::Unlabelled { issue, .. } => Some(*issue),
            Self::Adopt { .. } => None,
        }
    }
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// What the board picked, and what it passed over on the way
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// The ticket to dispatch, if any is free
    pub issue: Option<TicketId>,
    /// The ready issues older than it, or all of them when none is free
    pub skipped: Vec<Skip>,
}

/// Picks the ready issue that nobody is working on, first in `order`
///
/// `order` is the order the board keeps. An issue it omits comes after those
/// it lists, highest priority first and oldest first within a priority.
///
/// A ready issue labelled `in-progress` is passed over as held by an earlier
/// worker. The caller drops the issues its own work items hold before it asks,
/// so only an issue it does not hold reaches this check.
///
/// `finished` lists the issues whose work items kelpie already finished. The
/// forge can still list one as open and ready for a while after its pull
/// request merges, so the board never takes one again.
///
/// `dropped` lists the issues whose work items `drop` ended. The board leaves
/// one be while the forge lists it. The runner forgets one once the forge no
/// longer lists it, so a label put back returns it.
pub fn pick(
    ready: &[ReadyIssue],
    open: &[OpenPullRequest],
    finished: &[TicketId],
    dropped: &[TicketId],
    implementers: &[AgentName],
    order: &[TicketId],
) -> Pick {
    let mut ready: Vec<&ReadyIssue> = ready.iter().collect();
    ready.sort_by_key(|i| order_key(order, i));
    let mut skipped = Vec::new();
    for issue in ready {
        let number = issue.ticket;
        let mut by: Vec<TicketId> = issue
            .blocked_by
            .iter()
            .filter(|b| b.open)
            .map(|b| b.ticket)
            .collect();
        by.sort_unstable();
        if finished.contains(&number) {
            skipped.push(Skip::Finished { issue: number });
        } else if dropped.contains(&number) {
            skipped.push(Skip::Dropped { issue: number });
        } else if let Some(pr) = open.iter().find(|pr| closes(pr, number)) {
            skipped.push(Skip::PullRequest {
                issue: number,
                pull_request: pr.number,
            });
        } else if issue.assigned {
            skipped.push(Skip::Assigned { issue: number });
        } else if issue.labels.iter().any(|l| l == IN_PROGRESS) {
            skipped.push(Skip::InProgress { issue: number });
        } else if issue.sub_issues.total > 0 {
            skipped.push(Skip::Split {
                issue: number,
                open: issue
                    .sub_issues
                    .total
                    .saturating_sub(issue.sub_issues.closed),
            });
        } else if !by.is_empty() || issue.unlisted_blockers > 0 {
            skipped.push(Skip::Blocked {
                issue: number,
                by,
                unlisted: issue.unlisted_blockers,
            });
        } else if let Some(error) = label_error(&issue.labels, implementers) {
            skipped.push(Skip::Label {
                issue: number,
                error,
            });
        } else {
            return Pick {
                issue: Some(number),
                skipped,
            };
        }
    }
    Pick {
        issue: None,
        skipped,
    }
}

// Whether `pr` closes ticket `ticket`. A pull request names the GitHub issues
// its body resolves, which is how the forge lists it, and only a GitHub ticket
// has an issue number.
fn closes(pr: &OpenPullRequest, ticket: TicketId) -> bool {
    ticket
        .github()
        .is_some_and(|issue| pr.closes.contains(&issue))
}

/// The priority an issue's labels give it, from `P0` to `P4`, or `None` with none
pub fn priority(labels: &[String]) -> Option<&'static str> {
    let label = PRIORITIES.get(priority_rank(labels))?;
    label.strip_prefix(PRIORITY_LABEL)
}

/// The label `repo` has for `want`, ignoring case, or `None` where it has none
#[must_use]
pub fn repo_label<'a>(repo: &'a [String], want: &str) -> Option<&'a str> {
    repo.iter()
        .map(String::as_str)
        .find(|l| l.eq_ignore_ascii_case(want))
}

/// The label `repo` should carry for the priority `value`, which a triager
/// names `P2`: the board's own `priority: P2`, which [`priority`] reads,
/// looked for first, and the bare `P2` only on a repo that names its priorities
/// that way instead. `None` where the repo has neither, so a priority is never
/// put on as a label the repo does not use.
#[must_use]
pub fn repo_priority<'a>(repo: &'a [String], value: &str) -> Option<&'a str> {
    repo_label(repo, &format!("{PRIORITY_LABEL}{value}")).or_else(|| repo_label(repo, value))
}

/// Sorts `ready` into the board's order: `order` first, then an issue it
/// omits by the board rule, which is priority, then oldest by id
pub fn rule_order(ready: &mut [ReadyIssue], order: &[TicketId]) {
    ready.sort_by_key(|i| order_key(order, i));
}

/// The priority rank each listed ticket had when the board last read it, by
/// its id, so a changed label is seen
pub type Ranked = BTreeMap<TicketId, usize>;

/// The order and the ranks on record after a read of the board that listed
/// `ready`, given those before it
///
/// An issue the forge no longer lists, because it closed or lost
/// `ready-for-agent`, leaves the order. A newly ready issue goes after the
/// last issue of its priority that the order holds, wherever the maintainer
/// put it, or before every issue of a lower priority when it holds none. An
/// issue whose priority label changed since `ranked` has it moves the same
/// way, as if it were new. Several moves go in oldest first. An issue with no
/// rank on record keeps its place. With no order kept, the result is the board
/// rule's order.
pub fn keep_order(
    order: &[TicketId],
    ranked: &Ranked,
    ready: &[ReadyIssue],
) -> (Vec<TicketId>, Ranked) {
    let rank = |n: TicketId| {
        let issue = ready.iter().find(|i| i.ticket == n);
        issue.map(|i| priority_rank(&i.labels))
    };
    let mut kept: Vec<TicketId> = Vec::with_capacity(ready.len());
    for &n in order {
        let same = rank(n).is_some_and(|r| ranked.get(&n).is_none_or(|&was| was == r));
        if same && !kept.contains(&n) {
            kept.push(n);
        }
    }
    let mut moving: Vec<&ReadyIssue> = ready.iter().filter(|i| !kept.contains(&i.ticket)).collect();
    moving.sort_by_key(|i| i.ticket);
    for issue in moving {
        if kept.contains(&issue.ticket) {
            continue;
        }
        let mine = priority_rank(&issue.labels);
        let at = kept
            .iter()
            .rposition(|&n| rank(n).is_some_and(|r| r <= mine))
            .map_or(0, |p| p + 1);
        kept.insert(at, issue.ticket);
    }
    let ranks = kept
        .iter()
        .filter_map(|&n| rank(n).map(|r| (n, r)))
        .collect();
    (kept, ranks)
}

// Where the board puts an issue: where `order` lists it, an issue it omits
// after those it lists, then by the board rule.
fn order_key(order: &[TicketId], issue: &ReadyIssue) -> (usize, usize, TicketId) {
    let listed = order.iter().position(|&n| n == issue.ticket);
    (
        listed.unwrap_or(order.len()),
        priority_rank(&issue.labels),
        issue.ticket,
    )
}

// Where an issue's priority label puts it: 0 for P0 to 4 for P4, and 5 with
// none. With several, the highest counts.
fn priority_rank(labels: &[String]) -> usize {
    PRIORITIES
        .iter()
        .position(|p| labels.iter().any(|l| l == p))
        .unwrap_or(PRIORITIES.len())
}

// Why the board cannot take an issue with `labels`, given the project's
// implementers, before anything is spent on it.
fn label_error(labels: &[String], implementers: &[AgentName]) -> Option<LabelError> {
    agent_label(labels, implementers).err()
}

/// The implementer an issue's `agent:<name>` label names, if it has one
///
/// The prefix is read in any case, so `Agent:opus-high` is a label too, and
/// a `!` at its end, which pins the work item to it, is not the name's.
///
/// # Errors
///
/// [`LabelError`] when the issue has more than one `agent:` label, or its
/// label names no agent in `implementers`.
pub fn agent_label(
    labels: &[String],
    implementers: &[AgentName],
) -> Result<Option<AgentName>, LabelError> {
    let prefixed = |l: &&String| {
        l.get(..AGENT_LABEL.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(AGENT_LABEL))
    };
    let mut found = labels.iter().filter(prefixed);
    let Some(label) = found.next() else {
        return Ok(None);
    };
    if found.next().is_some() {
        return Err(LabelError::Several);
    }
    let value = &label[AGENT_LABEL.len()..];
    let value = value.strip_suffix(PIN).unwrap_or(value);
    match implementers.iter().find(|name| name.as_str() == value) {
        Some(name) => Ok(Some(name.clone())),
        None => Err(LabelError::NotListed {
            label: label.clone(),
            listed: implementers.iter().map(AgentName::to_string).collect(),
        }),
    }
}

/// Whether an issue's `agent:` label ends in `!`, which keeps its work item
/// on that implementer: it never falls back to another
pub fn pinned(labels: &[String]) -> bool {
    labels.iter().any(|l| {
        l.get(..AGENT_LABEL.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(AGENT_LABEL))
            && l.ends_with(PIN)
    })
}

/// An issue's `worker:` label, which kelpie no longer reads, if it has one
pub fn old_worker_label(labels: &[String]) -> Option<&str> {
    labels
        .iter()
        .find(|l| l.starts_with(OLD_WORKER_LABEL))
        .map(String::as_str)
}

/// Why an `agent:` label cannot be used
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabelError {
    /// The issue has more than one `agent:` label
    Several,
    /// The label names no agent the project lists in `agents.implementers`
    NotListed {
        /// The label
        label: String,
        /// The agents the project lists
        listed: Vec<String>,
    },
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Several => f.write_str("the issue has more than one `agent:` label"),
            Self::NotListed { label, listed } => write!(
                f,
                "label `{label}` names no agent the project lists in `agents.implementers`, \
                 which are {}",
                listed.join(", ")
            ),
        }
    }
}

impl core::error::Error for LabelError {}

#[cfg(test)]
mod tests;
