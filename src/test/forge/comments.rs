//! The App's own comments, which the forge lists beside what people wrote

use super::FakeForge;
use crate::test::github::FakeGithub;

impl FakeForge {
    /// Makes the forge list what `github`'s App posted on its issues and pull
    /// requests, as GitHub lists an App's comment beside anyone else's
    pub(crate) fn use_app_comments(&self, github: FakeGithub) {
        *self.app_comments.lock().unwrap() = Some(github);
    }
}
