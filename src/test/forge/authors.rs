//! The fake forge's pull request authors

use super::FakeForge;

impl FakeForge {
    /// Makes pull request `number` one `login` opened, not kelpie's account
    pub(crate) fn set_author(&self, number: u64, login: &str) {
        let mut prs = self.pull_requests.lock().unwrap();
        prs.get_mut(&number).expect("an opened pull request").author = login.to_owned();
    }

    /// Makes pull request `number` one a bot account opened
    pub(crate) fn set_author_bot(&self, number: u64) {
        let mut prs = self.pull_requests.lock().unwrap();
        prs.get_mut(&number)
            .expect("an opened pull request")
            .author_is_bot = true;
    }
}
