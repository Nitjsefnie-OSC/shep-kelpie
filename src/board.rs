//! The board: which ready issue becomes the next work item
//!
//! The board's input is the project's open issues labelled `ready-for-agent`.
//! The board keeps them in an order and dispatches the first that is free. A
//! newly ready issue joins it after the last issue of its priority: `priority: P0`,
//! then `P1`, `P2`, `P3`, `P4`, then an issue with no priority label, and within a
//! priority the oldest first, by issue number. An issue whose priority label
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

pub mod briefing;
mod open;

pub use open::{OpenPullRequest, ReviewMarks};

/// The label that puts an issue on the board
pub const READY: &str = "ready-for-agent";

/// The label of an issue nobody has read yet, which stays off the board
pub const TRIAGE: &str = "needs-triage";

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

/// An open issue labelled [`READY`], as the forge lists it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyIssue {
    /// Its number
    pub number: u64,
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
    /// The issue it is a sub-issue of, if any
    pub parent: Option<u64>,
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

/// An issue a ready issue is blocked by
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blocker {
    /// Its number
    pub number: u64,
    /// Whether the forge shows it open
    pub open: bool,
}

/// Why the board passed over a ready issue
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum Skip {
    /// An open pull request closes it
    PullRequest {
        /// The issue
        issue: u64,
        /// The pull request
        pull_request: u64,
    },
    /// Kelpie already finished a work item for it
    Finished {
        /// The issue
        issue: u64,
    },
    /// `drop` ended its work item, and the forge has listed it as ready since
    Dropped {
        /// The issue
        issue: u64,
    },
    /// Someone is assigned to it
    Assigned {
        /// The issue
        issue: u64,
    },
    /// It carries the `in-progress` label, so an earlier worker holds it
    InProgress {
        /// The issue
        issue: u64,
    },
    /// It has sub-issues, which are worked in its place
    Split {
        /// The issue
        issue: u64,
        /// Its sub-issues still open
        open: u64,
    },
    /// An issue it is blocked by is still open
    Blocked {
        /// The issue
        issue: u64,
        /// Its open blockers, lowest first
        by: Vec<u64>,
        /// Blockers the forge did not list, each taken as open
        #[serde(skip_serializing_if = "is_zero")]
        unlisted: u64,
    },
    /// Its `agent:` label names no listed implementer
    Label {
        /// The issue
        issue: u64,
        /// Why
        error: LabelError,
    },
    /// The forge could not show it when the runner came to take it
    Failed {
        /// The issue
        issue: u64,
        /// Why, as the refusal reads
        error: String,
    },
    /// Its pull request asked for a rework that could not start this poll
    Rework {
        /// The issue
        issue: u64,
        /// The pull request
        pull_request: u64,
        /// Why, as the refusal reads
        error: String,
    },
    /// Its body names a file the branch of a work item parked on a ruling
    /// changes, so it waits for that item to move
    Overlap {
        /// The issue
        issue: u64,
        /// The parked work item's issue
        with: u64,
        /// The files both touch
        files: Vec<String>,
    },
    /// It waits a pass for the board to read the paths its body names, or,
    /// with `unknown`, the files of that parked work item's branch, before
    /// the overlap with parked branches can be checked
    PathsUnread {
        /// The issue
        issue: u64,
        /// The parked work item whose branch's files are not known yet
        #[serde(skip_serializing_if = "Option::is_none")]
        unknown: Option<u64>,
    },
    /// It has no `agent:` label, and waits for the issue writer to pick
    /// one, or, with `ruling`, for the maintainer to answer that ruling
    Unlabelled {
        /// The issue
        issue: u64,
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
    /// The issue passed over, or 0 for an adopted pull request, which goes
    /// before every issue
    pub fn issue(&self) -> u64 {
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
            | Self::Unlabelled { issue, .. } => *issue,
            Self::Adopt { .. } => 0,
        }
    }
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// What the board picked, and what it passed over on the way
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// The issue to dispatch, if any is free
    pub issue: Option<u64>,
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
    finished: &[u64],
    dropped: &[u64],
    implementers: &[AgentName],
    order: &[u64],
) -> Pick {
    let mut ready: Vec<&ReadyIssue> = ready.iter().collect();
    ready.sort_by_key(|i| order_key(order, i));
    let mut skipped = Vec::new();
    for issue in ready {
        let number = issue.number;
        let mut by: Vec<u64> = issue
            .blocked_by
            .iter()
            .filter(|b| b.open)
            .map(|b| b.number)
            .collect();
        by.sort_unstable();
        if finished.contains(&number) {
            skipped.push(Skip::Finished { issue: number });
        } else if dropped.contains(&number) {
            skipped.push(Skip::Dropped { issue: number });
        } else if let Some(pr) = open.iter().find(|pr| pr.closes.contains(&number)) {
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

/// The priority an issue's labels give it, from `P0` to `P4`, or `None` with none
pub fn priority(labels: &[String]) -> Option<&'static str> {
    let label = PRIORITIES.get(priority_rank(labels))?;
    label.strip_prefix("priority: ")
}

/// Sorts `ready` into the board's order: `order` first, then an issue it
/// omits by the board rule, which is priority, then oldest by number
pub fn rule_order(ready: &mut [ReadyIssue], order: &[u64]) {
    ready.sort_by_key(|i| order_key(order, i));
}

/// The priority rank each listed issue had when the board last read it, by
/// issue number, so a changed label is seen
pub type Ranked = BTreeMap<u64, usize>;

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
pub fn keep_order(order: &[u64], ranked: &Ranked, ready: &[ReadyIssue]) -> (Vec<u64>, Ranked) {
    let rank = |n: u64| {
        let issue = ready.iter().find(|i| i.number == n);
        issue.map(|i| priority_rank(&i.labels))
    };
    let mut kept: Vec<u64> = Vec::with_capacity(ready.len());
    for &n in order {
        let same = rank(n).is_some_and(|r| ranked.get(&n).is_none_or(|&was| was == r));
        if same && !kept.contains(&n) {
            kept.push(n);
        }
    }
    let mut moving: Vec<&ReadyIssue> = ready.iter().filter(|i| !kept.contains(&i.number)).collect();
    moving.sort_by_key(|i| i.number);
    for issue in moving {
        if kept.contains(&issue.number) {
            continue;
        }
        let mine = priority_rank(&issue.labels);
        let at = kept
            .iter()
            .rposition(|&n| rank(n).is_some_and(|r| r <= mine))
            .map_or(0, |p| p + 1);
        kept.insert(at, issue.number);
    }
    let ranks = kept
        .iter()
        .filter_map(|&n| rank(n).map(|r| (n, r)))
        .collect();
    (kept, ranks)
}

// Where the board puts an issue: where `order` lists it, an issue it omits
// after those it lists, then by the board rule.
fn order_key(order: &[u64], issue: &ReadyIssue) -> (usize, usize, u64) {
    let listed = order.iter().position(|&n| n == issue.number);
    (
        listed.unwrap_or(order.len()),
        priority_rank(&issue.labels),
        issue.number,
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
mod tests {
    use super::*;
    use crate::ports::Checks;

    fn ready(number: u64) -> ReadyIssue {
        ReadyIssue {
            number,
            title: String::new(),
            body: String::new(),
            assigned: false,
            labels: vec![READY.into()],
            blocked_by: vec![],
            unlisted_blockers: 0,
            parent: None,
            sub_issues: SubIssues::default(),
        }
    }

    fn blocked(number: u64, by: &[(u64, bool)]) -> ReadyIssue {
        let blocked_by = by.iter().map(|&(number, open)| Blocker { number, open });
        ReadyIssue {
            blocked_by: blocked_by.collect(),
            ..ready(number)
        }
    }

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    // A project listing `sonnet-high` then `opus-high`.
    fn listed() -> Vec<AgentName> {
        ["sonnet-high", "opus-high"]
            .map(|n| AgentName::try_from(n.to_owned()).unwrap())
            .to_vec()
    }

    #[test]
    fn the_oldest_ready_issue_is_picked_whatever_order_they_are_listed_in() {
        let pick = pick(
            &[ready(16), ready(12), ready(14)],
            &[],
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(12));
        assert_eq!(pick.skipped, []);
    }

    fn prioritised(number: u64, priority: &str) -> ReadyIssue {
        let mut issue = ready(number);
        issue.labels.push(priority.into());
        issue
    }

    #[test]
    fn pick_follows_the_order_over_the_priority_labels() {
        let issues = [prioritised(9, "priority: P0"), ready(2), ready(5)];
        let pick = pick(&issues, &[], &[], &[], &listed(), &[5, 9, 2]);
        assert_eq!(pick.issue, Some(5));
        assert_eq!(pick.skipped, []);
    }

    #[test]
    fn pick_passes_a_skipped_issue_over_to_the_next_in_the_order() {
        let issues = [
            ready(3),
            blocked(8, &[(32, true)]),
            prioritised(9, "priority: P3"),
        ];
        let pick = pick(&issues, &[], &[], &[], &listed(), &[8, 9, 3]);
        assert_eq!(pick.issue, Some(9));
        assert_eq!(pick.skipped.len(), 1);
    }

    #[test]
    fn rule_order_follows_the_order_and_puts_an_issue_it_omits_last_by_the_rule() {
        let mut issues = vec![
            ready(1),
            prioritised(2, "priority: P0"),
            prioritised(3, "priority: P0"),
            ready(4),
        ];
        rule_order(&mut issues, &[4, 3]);
        let numbers: Vec<u64> = issues.iter().map(|i| i.number).collect();
        assert_eq!(numbers, [4, 3, 2, 1]);
    }

    #[test]
    fn with_no_order_kept_the_order_is_the_board_rule() {
        let issues = [
            ready(1),
            prioritised(2, "priority: P3"),
            prioritised(3, "priority: P1"),
            ready(4),
            prioritised(5, "priority: P0"),
            prioritised(6, "priority: P2"),
            prioritised(7, "priority: P1"),
            prioritised(8, "priority: P0"),
            prioritised(9, "priority: P4"),
        ];
        assert_eq!(kept(&[], &issues), [5, 8, 3, 7, 6, 2, 9, 1, 4]);
    }

    #[test]
    fn a_newly_ready_p0_lands_after_the_last_p0_and_before_the_first_p1() {
        let issues = [
            prioritised(1, "priority: P0"),
            prioritised(2, "priority: P0"),
            prioritised(3, "priority: P1"),
            prioritised(7, "priority: P0"),
        ];
        assert_eq!(kept(&[1, 2, 3], &issues), [1, 2, 7, 3]);
    }

    #[test]
    fn a_newly_ready_issue_goes_after_the_last_of_its_priority_wherever_the_order_put_it() {
        let issues = [
            prioritised(1, "priority: P0"),
            prioritised(2, "priority: P1"),
            prioritised(3, "priority: P0"),
            ready(4),
            prioritised(9, "priority: P0"),
        ];
        // #1 sits below #2, so it is the last P0, and #3 and #9 follow it.
        assert_eq!(kept(&[2, 1, 4], &issues), [2, 1, 3, 9, 4]);
    }

    #[test]
    fn a_closed_or_unlabelled_issue_leaves_the_order() {
        // The forge lists only open issues labelled `ready-for-agent`, so #2
        // is missing from `issues` once it closes or loses the label.
        let issues = [ready(1), ready(3)];
        assert_eq!(kept(&[1, 2, 3], &issues), [1, 3]);
    }

    // The order after a read, when no issue has a priority on record yet.
    fn kept(order: &[u64], issues: &[ReadyIssue]) -> Vec<u64> {
        keep_order(order, &Ranked::new(), issues).0
    }

    // The order and the priorities on record after a first read of `issues`.
    fn first_read(issues: &[ReadyIssue]) -> (Vec<u64>, Ranked) {
        keep_order(&[], &Ranked::new(), issues)
    }

    #[test]
    fn a_relabelled_issue_moves_to_after_the_last_issue_of_its_new_priority() {
        let mut issues = vec![
            prioritised(1, "priority: P0"),
            prioritised(2, "priority: P0"),
            prioritised(3, "priority: P1"),
            prioritised(4, "priority: P1"),
        ];
        let (order, ranked) = first_read(&issues);
        assert_eq!(order, [1, 2, 3, 4]);

        // #4 is bumped to P0 and #1 dropped to P2.
        issues[3].labels = labels(&[READY, "priority: P0"]);
        issues[0].labels = labels(&[READY, "priority: P2"]);
        let (order, ranked) = keep_order(&order, &ranked, &issues);
        assert_eq!(order, [2, 4, 3, 1]);

        // Read again with nothing changed, the order stands.
        assert_eq!(keep_order(&order, &ranked, &issues).0, [2, 4, 3, 1]);
    }

    #[test]
    fn an_issue_whose_priority_is_not_on_record_keeps_its_place() {
        let issues = [
            prioritised(1, "priority: P3"),
            prioritised(2, "priority: P0"),
        ];
        let (order, ranked) = keep_order(&[1, 2], &Ranked::new(), &issues);
        assert_eq!(order, [1, 2]);
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn a_p0_issue_is_picked_before_an_older_unlabelled_one() {
        let pick = pick(
            &[ready(3), prioritised(9, "priority: P0")],
            &[],
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(9));
        assert_eq!(pick.skipped, []);
    }

    #[test]
    fn priorities_order_the_board_p0_to_p3_then_unlabelled_oldest_first_within_each() {
        let mut order = Vec::new();
        let mut issues = vec![
            ready(1),
            prioritised(2, "priority: P3"),
            prioritised(3, "priority: P1"),
            ready(4),
            prioritised(5, "priority: P0"),
            prioritised(6, "priority: P2"),
            prioritised(7, "priority: P1"),
            prioritised(8, "priority: P0"),
        ];
        while let Some(next) = pick(&issues, &[], &[], &[], &listed(), &[]).issue {
            order.push(next);
            issues.retain(|i| i.number != next);
        }
        assert_eq!(order, [5, 8, 3, 7, 6, 2, 1, 4]);
    }

    #[test]
    fn an_issue_with_several_priority_labels_ranks_by_the_highest() {
        let mut both = prioritised(9, "priority: P3");
        both.labels.push("priority: P1".into());
        let pick = pick(
            &[prioritised(2, "priority: P2"), both],
            &[],
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(9));
    }

    #[test]
    fn a_blocked_p0_issue_waits_and_the_next_priority_is_picked() {
        let mut p0 = blocked(9, &[(4, true)]);
        p0.labels.push("priority: P0".into());
        let pick = pick(&[p0, ready(1)], &[], &[], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(1));
        assert_eq!(pick.skipped.len(), 1);
    }

    #[test]
    fn an_issue_with_sub_issues_is_never_picked_while_its_sub_issues_are() {
        let mut parent = ready(4);
        parent.sub_issues = SubIssues {
            total: 3,
            closed: 1,
        };
        let mut piece = ready(5);
        piece.parent = Some(4);
        let pick = pick(&[parent, piece], &[], &[], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(5));
        assert_eq!(pick.skipped, [Skip::Split { issue: 4, open: 2 }]);
    }

    #[test]
    fn nothing_ready_picks_nothing() {
        assert_eq!(
            pick(&[], &[], &[], &[], &listed(), &[]),
            Pick {
                issue: None,
                skipped: vec![]
            }
        );
    }

    #[test]
    fn an_issue_with_an_open_pull_request_or_an_assignee_is_passed_over() {
        let mut taken = ready(3);
        taken.assigned = true;
        let open = [OpenPullRequest {
            number: 40,
            head: "feat/2".into(),
            closes: vec![2],
            labels: vec![],
            draft: false,
            commit: "c0ffee".into(),
            checks: Checks::None,
            from_fork: false,
            author: "someone".into(),
            author_is_bot: false,
            marks: ReviewMarks::default(),
        }];
        let pick = pick(
            &[ready(2), taken, ready(5)],
            &open,
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(5));
        assert_eq!(
            pick.skipped,
            [
                Skip::PullRequest {
                    issue: 2,
                    pull_request: 40
                },
                Skip::Assigned { issue: 3 },
            ]
        );
    }

    #[test]
    fn an_issue_an_earlier_worker_labelled_in_progress_is_passed_over_for_the_next_oldest() {
        let mut held = ready(2);
        held.labels.push(IN_PROGRESS.into());
        let pick = pick(&[held, ready(5)], &[], &[], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(5));
        assert_eq!(pick.skipped, [Skip::InProgress { issue: 2 }]);
    }

    #[test]
    fn an_issue_kelpie_finished_is_passed_over_while_the_forge_still_lists_it() {
        let pick = pick(&[ready(22), ready(23)], &[], &[22], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(23));
        assert_eq!(pick.skipped, [Skip::Finished { issue: 22 }]);
    }

    #[test]
    fn an_issue_kelpie_dropped_is_passed_over_as_dropped_and_not_as_finished() {
        let pick = pick(&[ready(22), ready(23)], &[], &[], &[22], &listed(), &[]);
        assert_eq!(pick.issue, Some(23));
        assert_eq!(pick.skipped, [Skip::Dropped { issue: 22 }]);
    }

    #[test]
    fn a_board_where_every_issue_is_taken_picks_nothing_and_says_why() {
        let mut taken = ready(3);
        taken.assigned = true;
        assert_eq!(
            pick(&[taken], &[], &[], &[], &listed(), &[]),
            Pick {
                issue: None,
                skipped: vec![Skip::Assigned { issue: 3 }]
            }
        );
    }

    #[test]
    fn an_issue_labelled_for_an_agent_the_project_does_not_list_is_passed_over() {
        let mut odd = ready(1);
        odd.labels.push("agent:haiku-low".into());
        let pick = pick(&[odd, ready(2)], &[], &[], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(2));
        assert_eq!(
            pick.skipped,
            [Skip::Label {
                issue: 1,
                error: LabelError::NotListed {
                    label: "agent:haiku-low".into(),
                    listed: vec!["sonnet-high".into(), "opus-high".into()],
                }
            }]
        );
        let Skip::Label { error, .. } = &pick.skipped[0] else {
            panic!("{:?}", pick.skipped);
        };
        assert_eq!(
            error.to_string(),
            "label `agent:haiku-low` names no agent the project lists in \
             `agents.implementers`, which are sonnet-high, opus-high"
        );
    }

    #[test]
    fn an_old_worker_label_is_no_label_error() {
        let mut old = ready(1);
        old.labels.push("worker:gpt-high".into());
        assert_eq!(
            pick(&[old, ready(2)], &[], &[], &[], &listed(), &[]).issue,
            Some(1)
        );
    }

    #[test]
    fn an_issue_with_an_open_blocker_is_passed_over_for_the_next_oldest() {
        let pick = pick(
            &[blocked(8, &[(32, true)]), ready(9)],
            &[],
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(9));
        assert_eq!(
            pick.skipped,
            [Skip::Blocked {
                issue: 8,
                by: vec![32],
                unlisted: 0
            }]
        );
    }

    #[test]
    fn an_issue_whose_blockers_are_all_closed_is_picked() {
        let pick = pick(
            &[blocked(8, &[(32, false), (33, false)])],
            &[],
            &[],
            &[],
            &listed(),
            &[],
        );
        assert_eq!(pick.issue, Some(8));
        assert_eq!(pick.skipped, []);
    }

    #[test]
    fn a_skip_for_blockers_names_only_the_open_ones_lowest_first() {
        let mixed = blocked(27, &[(40, true), (24, false), (19, true), (37, true)]);
        assert_eq!(
            pick(&[mixed], &[], &[], &[], &listed(), &[]),
            Pick {
                issue: None,
                skipped: vec![Skip::Blocked {
                    issue: 27,
                    by: vec![19, 37, 40],
                    unlisted: 0
                }]
            }
        );
    }

    #[test]
    fn blockers_the_forge_did_not_list_count_as_open() {
        let mut long = blocked(8, &[(12, false)]);
        long.unlisted_blockers = 3;
        let pick = pick(&[long, ready(9)], &[], &[], &[], &listed(), &[]);
        assert_eq!(pick.issue, Some(9));
        assert_eq!(
            pick.skipped,
            [Skip::Blocked {
                issue: 8,
                by: vec![],
                unlisted: 3
            }]
        );
        let json = serde_json::to_value(&pick.skipped[0]).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "reason": "blocked", "issue": 8, "by": [], "unlisted": 3 })
        );
    }

    #[test]
    fn an_agent_label_names_a_listed_implementer() {
        let opus = AgentName::try_from("opus-high".to_owned()).unwrap();
        assert_eq!(
            agent_label(&labels(&[READY, "agent:opus-high"]), &listed()),
            Ok(Some(opus))
        );
        assert_eq!(agent_label(&labels(&[READY, "bug"]), &listed()), Ok(None));
        assert_eq!(
            agent_label(&labels(&["worker:opus-high"]), &listed()),
            Ok(None)
        );
        let pin = labels(&[READY, "Agent:opus-high!"]);
        assert_eq!(
            agent_label(&pin, &listed()).unwrap().unwrap().as_str(),
            "opus-high"
        );
        assert!(pinned(&pin));
        assert!(!pinned(&labels(&[READY, "agent:opus-high", "bug!"])));
        for bad in [
            "agent:",
            "agent:Opus-high",
            "agent:opus-low",
            "agent:!",
            "agent:opus-high!!",
        ] {
            assert!(
                matches!(
                    agent_label(&labels(&[bad]), &listed()),
                    Err(LabelError::NotListed { label, .. }) if label == bad
                ),
                "{bad}"
            );
        }
        assert_eq!(
            agent_label(
                &labels(&["agent:opus-high", "agent:sonnet-high"]),
                &listed()
            ),
            Err(LabelError::Several)
        );
    }

    #[test]
    fn an_old_worker_label_is_found_to_be_told_about() {
        let found = labels(&[READY, "worker:opus-medium"]);
        assert_eq!(old_worker_label(&found), Some("worker:opus-medium"));
        assert_eq!(old_worker_label(&labels(&[READY, "agent:x"])), None);
    }
}
