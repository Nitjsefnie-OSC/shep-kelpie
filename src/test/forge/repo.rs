//! The fake forge's repo-wide answers: its visibility, whether the account may
//! push, its default branch and labels, and who the account is

use std::sync::atomic::Ordering;

use super::FakeForge;
use crate::ports::{ForgeError, Visibility};
use crate::settings::ForgeSlug;

impl FakeForge {
    pub(crate) fn set_visibility(&self, visibility: Visibility) {
        *self.visibility.lock().unwrap() = visibility;
    }

    /// Makes `repo` alone `visibility`, whatever the rest are
    pub(crate) fn set_visibility_of(&self, repo: &ForgeSlug, visibility: Visibility) {
        let key = repo.as_str().to_lowercase();
        self.visibilities.lock().unwrap().insert(key, visibility);
    }

    /// Whether the account may push to the repo, which it may until a test says not
    pub(crate) fn set_can_push(&self, pushes: bool) {
        self.pushes.store(pushes, Ordering::SeqCst);
    }

    /// Whether a review bot has ever commented on the repo, which it has until a test says not
    pub(crate) fn set_review_bot_seen(&self, seen: bool) {
        self.bot_seen.store(seen, Ordering::SeqCst);
    }

    /// Makes asking who the account is fail with `error`, as `gh` does when it is logged out
    pub(crate) fn set_viewer_down(&self, error: ForgeError) {
        *self.viewer_down.lock().unwrap() = Some(error);
    }

    /// Lets asking who the account is succeed again, after [`Self::set_viewer_down`]
    pub(crate) fn set_viewer_up(&self) {
        *self.viewer_down.lock().unwrap() = None;
    }

    pub(crate) fn set_default_branch(&self, branch: &str) {
        *self.default_branch.lock().unwrap() = branch.to_owned();
    }

    /// The repo's labels, those it started with and those made since
    pub(crate) fn repo_labels_now(&self) -> Vec<String> {
        self.repo_labels.lock().unwrap().clone()
    }

    pub(crate) fn set_repo_labels(&self, labels: &[&str]) {
        *self.repo_labels.lock().unwrap() = labels.iter().map(|&l| l.to_owned()).collect();
    }
}
