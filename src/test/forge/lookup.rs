//! The fake forge's lookups of one issue: its labels, blockers, parent and
//! sub-issue count

use super::FakeForge;
use crate::board::{Blocker, SubIssues};

impl FakeForge {
    pub(super) fn labels_of(&self, number: u64) -> Vec<String> {
        let labels = self.labels.lock().unwrap();
        labels.get(&number).cloned().unwrap_or_default()
    }

    pub(super) fn blockers_of(&self, number: u64) -> Vec<Blocker> {
        let blockers = self.blockers.lock().unwrap();
        let closed = self.closed.lock().unwrap();
        let by = blockers.get(&number).map(Vec::as_slice).unwrap_or_default();
        by.iter()
            .map(|&number| Blocker {
                number,
                open: !closed.contains(&number),
            })
            .collect()
    }

    pub(super) fn parent_of(&self, number: u64) -> Option<u64> {
        let links = self.sub_issues.lock().unwrap();
        links
            .iter()
            .find(|(_, children)| children.contains(&number))
            .map(|(&parent, _)| parent)
    }

    pub(super) fn count_sub_issues(&self, parent: u64) -> SubIssues {
        let children = self.sub_issues_of(parent);
        let closed = self.closed.lock().unwrap();
        SubIssues {
            total: children.len() as u64,
            closed: children.iter().filter(|c| closed.contains(c)).count() as u64,
        }
    }
}
