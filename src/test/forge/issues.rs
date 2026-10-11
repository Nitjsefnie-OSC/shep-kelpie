//! The fake forge's open issues, the ones kelpie opened, and the issues' calls going down

use super::{CreatedIssue, FakeForge};
use crate::ports::OpenIssue;

impl FakeForge {
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
}
