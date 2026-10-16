//! The fake forge's pull requests and the board's ready issues, which a test
//! lists, labels and blocks as another person would

use super::FakeForge;
use crate::board::{READY, ReadyIssue, SubIssues};

impl FakeForge {
    /// Adds `label` to pull request `number`, as someone other than kelpie would
    pub(crate) fn label_pull_request(&self, number: u64, label: &str) {
        self.coderabbit.put_label(number, label, true);
    }

    /// Pull request `number`'s labels, in the order they went on
    pub(crate) fn pull_request_labels(&self, number: u64) -> Vec<String> {
        self.coderabbit.labels(number)
    }

    /// Removes `label` from pull request `number`, as someone other than kelpie would
    pub(crate) fn unlabel_pull_request(&self, number: u64, label: &str) {
        self.coderabbit.put_label(number, label, false);
    }

    /// Lists issue `number` as ready, with whether anyone is assigned
    pub(crate) fn list_ready(&self, number: u64, assigned: bool) {
        self.label(number, READY);
        self.ready.lock().unwrap().push(ReadyIssue {
            number,
            title: format!("Title of #{number}"),
            body: format!("Body of #{number}.\n"),
            assigned,
            labels: Vec::new(),
            blocked_by: Vec::new(),
            unlisted_blockers: 0,
            parent: None,
            sub_issues: SubIssues::default(),
        });
    }

    /// Takes `ready-for-agent` off issue `number`, so the board no longer lists it
    pub(crate) fn unlist_ready(&self, number: u64) {
        self.ready.lock().unwrap().retain(|i| i.number != number);
        let mut labels = self.labels.lock().unwrap();
        labels.entry(number).or_default().retain(|l| l != READY);
    }

    /// Gives ready issue `number` the body `body`, as its author wrote it
    pub(crate) fn set_ready_body(&self, number: u64, body: &str) {
        let mut ready = self.ready.lock().unwrap();
        let issue = ready.iter_mut().find(|i| i.number == number).unwrap();
        issue.body = body.to_owned();
    }

    /// The issues issue `number` is blocked by, open or closed
    pub(crate) fn blockers(&self, number: u64) -> Vec<u64> {
        self.blockers_of(number).iter().map(|b| b.number).collect()
    }

    /// Marks issue `number` blocked by issue `by`, which is open until closed
    pub(crate) fn block(&self, number: u64, by: u64) {
        let mut blockers = self.blockers.lock().unwrap();
        blockers.entry(number).or_default().push(by);
    }

    /// Closes issue `number`, as merging its pull request or someone else would
    pub(crate) fn close_issue(&self, number: u64) {
        self.closed.lock().unwrap().insert(number);
    }
}
