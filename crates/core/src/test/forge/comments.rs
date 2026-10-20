//! The App's own comments, which the forge lists beside what people wrote

use std::fmt;
use std::sync::Arc;

use super::FakeForge;
use crate::ports::ThreadComment;

/// An App that posts comments, which the forge lists beside what people wrote
pub trait AppComments: fmt::Debug + Send + Sync {
    /// Every comment the App posted on issue or pull request `number`
    fn posted_on(&self, number: u64) -> Vec<ThreadComment>;
}

impl FakeForge {
    /// Makes the forge list what `github`'s App posted on its issues and pull
    /// requests, as GitHub lists an App's comment beside anyone else's
    pub fn use_app_comments(&self, github: Arc<dyn AppComments>) {
        *self.app_comments.lock().unwrap() = Some(github);
    }
}
