//! The fake forge's open issues, the ones kelpie opened and closed, and the issues' calls going down

use std::sync::atomic::Ordering;

use super::FakeForge;
use crate::ports::{FinishedParent, ForgeError, OpenIssue, SubIssue, SubIssueState};

/// An issue kelpie opened on the fake forge
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedIssue {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) labels: Vec<String>,
}

impl FakeForge {
    /// Makes issue `child` a sub-issue of issue `parent`, as someone else would
    pub(crate) fn link_sub_issue(&self, parent: u64, child: u64) {
        let mut links = self.sub_issues.lock().unwrap();
        links.entry(parent).or_default().push(child);
    }

    /// Closes issue `number` as not planned, as someone else would
    pub(crate) fn close_issue_as_not_planned(&self, number: u64) {
        self.not_planned.lock().unwrap().insert(number);
        self.close_issue(number);
    }

    /// Makes listing a parent's sub-issues fail, or work again
    pub(crate) fn set_sub_issues_down(&self, down: bool) {
        self.sub_issues_down.store(down, Ordering::SeqCst);
    }

    /// Issue `parent`'s sub-issues, in the order they were linked
    pub(crate) fn sub_issues_of(&self, parent: u64) -> Vec<u64> {
        let links = self.sub_issues.lock().unwrap();
        links.get(&parent).cloned().unwrap_or_default()
    }

    pub(super) fn read_sub_issues(&self, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
        self.ask()?;
        if self.sub_issues_down.load(Ordering::SeqCst) {
            return Err(ForgeError::Failed("sub-issues are unreadable".into()));
        }
        let closed = self.closed.lock().unwrap();
        let not_planned = self.not_planned.lock().unwrap();
        Ok(self
            .sub_issues_of(parent)
            .into_iter()
            .map(|number| SubIssue {
                number,
                state: match (closed.contains(&number), not_planned.contains(&number)) {
                    (false, _) => SubIssueState::Open,
                    (true, true) => SubIssueState::NotPlanned,
                    (true, false) => SubIssueState::Completed,
                },
            })
            .collect())
    }

    /// Makes the listing of finished parents name `parent` from now on, as a
    /// stale listing would, whether or not its sub-issues are all closed
    pub(crate) fn list_finished_whatever_its_sub_issues(&self, parent: u64) {
        self.listed_finished.lock().unwrap().push(parent);
    }

    /// Shows closed issue `number` as completed, as someone else changing the
    /// reason it closed with would
    pub(crate) fn reclose_as_completed(&self, number: u64) {
        self.not_planned.lock().unwrap().remove(&number);
    }

    pub(super) fn read_finished_parents(&self) -> Result<Vec<FinishedParent>, ForgeError> {
        self.ask()?;
        let mut parents: Vec<_> = self.sub_issues.lock().unwrap().keys().copied().collect();
        parents.sort_unstable();
        let stale = self.listed_finished.lock().unwrap().clone();
        let closed = self.closed.lock().unwrap();
        Ok(parents
            .into_iter()
            .filter(|parent| !closed.contains(parent))
            .map(|number| (number, self.sub_issues_of(number)))
            .filter(|(n, subs)| stale.contains(n) || subs.iter().all(|s| closed.contains(s)))
            .map(|(number, subs)| FinishedParent {
                number,
                sub_issues: subs.len() as u64,
            })
            .collect())
    }

    /// Leaves issue `number` open, as someone other than kelpie would have,
    /// and shows this title and body when it is viewed
    pub(crate) fn open_issue(&self, number: u64, title: &str, body: &str) {
        self.issues.lock().unwrap().push(OpenIssue {
            number,
            title: title.to_owned(),
            body: body.to_owned(),
        });
    }

    /// Every issue kelpie opened, oldest first
    pub(crate) fn created(&self) -> Vec<CreatedIssue> {
        self.created.lock().unwrap().clone()
    }

    /// Makes listing and opening issues fail, or work again
    pub(crate) fn set_issues_down(&self, down: bool) {
        let why = down.then(|| "issues are down".to_owned());
        *self.issues_down.lock().unwrap() = why;
    }

    /// Makes listing and opening issues fail with `error`, verbatim
    pub(crate) fn set_issues_error(&self, error: &str) {
        *self.issues_down.lock().unwrap() = Some(error.to_owned());
    }

    /// Lets `n` more issues open, then makes every later one fail
    pub(crate) fn set_creates_left(&self, n: usize) {
        *self.creates_left.lock().unwrap() = Some(n);
    }

    /// Makes closing an issue fail, or work again
    pub(crate) fn set_closes_down(&self, down: bool) {
        self.closes_down.store(down, Ordering::SeqCst);
    }

    /// How many closes the forge refused while closing was down
    pub(crate) fn closes_refused(&self) -> usize {
        self.closes_refused.load(Ordering::SeqCst)
    }

    /// The issues kelpie closed
    pub(crate) fn closings(&self) -> Vec<u64> {
        self.closings.lock().unwrap().clone()
    }
}
