//! Which issues other people opened are due for triage
//!
//! While `triage.pick` is not `off`, the runner lists the open issues once a
//! [`BOARD_POLL`](super::BOARD_POLL), and `status` shows the untouched ones
//! under `triage.due`, oldest first. An issue is untouched when it carries
//! none of [`TRIAGE_LABELS`], no bot opened it, it was opened after
//! `triage.since`, and, under `maintainer`, the maintainer opened it.
//! Nothing is posted or labelled here.

use serde::Serialize;

use super::Runner;
use crate::ports::{OpenIssue, Timestamp};
use crate::settings::{Settings, TriagePick};
use crate::state::{ProjectState, TriageState};

/// The labels that say an issue has been looked at, so triage leaves it alone
pub(super) const TRIAGE_LABELS: [&str; 5] = [
    "needs-triage",
    "needs-info",
    "ready-for-agent",
    "ready-for-human",
    "wontfix",
];

/// An untouched issue, as `status` lists it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DueStatus {
    /// Its number
    pub issue: u64,
    /// Its title
    pub title: String,
    /// The login of whoever opened it, empty when their account is gone
    pub author: String,
    /// When it was opened
    pub created: Timestamp,
}

/// What `status` says of triage
#[derive(Debug, Serialize)]
pub struct TriageStatus<'a> {
    /// The untouched issues, oldest first
    pub due: &'a [DueStatus],
}

/// Sets `state.triage` for a start under `settings`: stamped `now` when
/// `triage.pick` is not `off` and none is saved, cleared when it is `off`.
/// Returns whether it changed.
pub(super) fn settle_since(state: &mut ProjectState, settings: &Settings, now: Timestamp) -> bool {
    match (settings.triage.is_off(), &state.triage) {
        (true, Some(_)) => state.triage = None,
        (false, None) => state.triage = Some(TriageState { since: now }),
        _ => return false,
    }
    true
}

impl Runner {
    // Lists the untouched issues once a poll while `triage.pick` is not `off`.
    // A listing that cannot be read leaves the last one as it was.
    pub(super) fn follow_triage(&mut self) {
        let Some(since) = self.state.triage.as_ref().map(|t| t.since) else {
            return;
        };
        let now = self.ports.clock.now();
        if !self.looks.triage_due(now) {
            return;
        }
        let Ok(open) = self.ports.forge.open_issues(&self.remote) else {
            return;
        };
        let only = match self.settings.triage.pick {
            TriagePick::Maintainer => match self.maintainer() {
                Ok(login) => Some(login),
                Err(_) => return,
            },
            TriagePick::All | TriagePick::Off => None,
        };
        let mut due: Vec<&OpenIssue> = (open.iter())
            .filter(|issue| untouched(issue, since, only.as_deref()))
            .collect();
        due.sort_by_key(|issue| (issue.created, issue.number));
        self.triage_due = due
            .into_iter()
            .map(|issue| DueStatus {
                issue: issue.number,
                title: issue.title.clone(),
                author: issue.author.clone(),
                created: issue.created,
            })
            .collect();
    }

    // What `status` says of triage, or none while no issue is due
    pub(super) fn triage_status(&self) -> Option<TriageStatus<'_>> {
        (!self.triage_due.is_empty()).then_some(TriageStatus {
            due: &self.triage_due,
        })
    }
}

// Whether `issue` is untouched, opened after `since`, and by `only` when one is named
fn untouched(issue: &OpenIssue, since: Timestamp, only: Option<&str>) -> bool {
    let labelled = (issue.labels.iter()).any(|l| TRIAGE_LABELS.contains(&l.as_str()));
    !labelled
        && !issue.author_is_bot
        && issue.created > since
        && only.is_none_or(|login| issue.author.eq_ignore_ascii_case(login))
}

#[cfg(test)]
mod pick_tests;
