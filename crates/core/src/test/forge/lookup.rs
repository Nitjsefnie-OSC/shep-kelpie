//! The fake forge's lookups of one issue: its labels, blockers, parent and
//! sub-issue count

use super::FakeForge;
use crate::board::{Blocker, SubIssues};
use crate::test::ticket;

impl FakeForge {
    /// Issue `number`'s labels, in the order they went on
    pub fn issue_labels(&self, number: u64) -> Vec<String> {
        self.labels_of(number)
    }

    /// Labels issue `number` with `label`, on the board and when viewed
    pub fn label(&self, number: u64, label: &str) {
        let mut labels = self.labels.lock().unwrap();
        labels.entry(number).or_default().push(label.to_owned());
    }

    /// The labels the stand-in holds on `number`
    pub fn labels_of(&self, number: u64) -> Vec<String> {
        let labels = self.labels.lock().unwrap();
        labels.get(&number).cloned().unwrap_or_default()
    }

    /// The blockers the stand-in holds on `number`
    pub fn blockers_of(&self, number: u64) -> Vec<Blocker> {
        let blockers = self.blockers.lock().unwrap();
        let closed = self.closed.lock().unwrap();
        let by = blockers.get(&number).map(Vec::as_slice).unwrap_or_default();
        by.iter()
            .map(|&number| Blocker {
                ticket: ticket(number),
                open: !closed.contains(&number),
            })
            .collect()
    }

    /// The parent the stand-in holds for `number`, if any
    pub fn parent_of(&self, number: u64) -> Option<u64> {
        let links = self.sub_issues.lock().unwrap();
        links
            .iter()
            .find(|(_, children)| children.contains(&number))
            .map(|(&parent, _)| parent)
    }

    /// How many sub-issues the stand-in holds under `parent`
    pub fn count_sub_issues(&self, parent: u64) -> SubIssues {
        let children = self.sub_issues_of(parent);
        let closed = self.closed.lock().unwrap();
        SubIssues {
            total: children.len() as u64,
            closed: children.iter().filter(|c| closed.contains(c)).count() as u64,
        }
    }
}
