//! The guard as the ticket port, and as both ports at once (ADR 0014)
//!
//! A GitHub project's tickets are its issues, so a ticket read or written
//! through the guard is the guard's own forge call: held, checked against this
//! machine's paths and ledged exactly as the same call through the forge is.
//! The impls stand here rather than in `local_paths` itself, so no call site
//! there has both ports in scope at once.

use std::sync::Arc;

use super::Guarded;
use crate::board::ReadyIssue;
use crate::ports::{
    FinishedParent, Forge, ForgeError, Issue, IssueCloseReason, OpenIssue, SubIssue, ThreadComment,
    Tickets,
};
use crate::settings::ForgeSlug;

impl Guarded {
    /// This guard as both ports a runner reads the forge through: one guard
    /// behind the forge port and behind the ticket port, so a call on either
    /// reaches the same guard, rate-limit hold and ledger
    #[must_use]
    pub fn into_ports(self) -> (Arc<dyn Forge>, Arc<dyn Tickets>) {
        let guarded = Arc::new(self);
        (
            Arc::clone(&guarded) as Arc<dyn Forge>,
            Arc::clone(&guarded) as Arc<dyn Tickets>,
        )
    }
}

impl Tickets for Guarded {
    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError> {
        Forge::issue(self, repo, number)
    }

    fn sub_issues(&self, repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
        Forge::sub_issues(self, repo, parent)
    }

    fn finished_parents(&self, repo: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError> {
        Forge::finished_parents(self, repo)
    }

    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        Forge::ready_issues(self, repo)
    }

    fn open_issues(&self, repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
        Forge::open_issues(self, repo)
    }

    fn issue_comments(
        &self,
        repo: &ForgeSlug,
        number: u64,
    ) -> Result<Vec<ThreadComment>, ForgeError> {
        Forge::issue_comments(self, repo, number)
    }

    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        Forge::create_issue(self, repo, title, body, labels)
    }

    fn close_issue(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        Forge::close_issue(self, repo, number)
    }

    fn close_issue_as(
        &self,
        repo: &ForgeSlug,
        number: u64,
        reason: IssueCloseReason,
    ) -> Result<(), ForgeError> {
        Forge::close_issue_as(self, repo, number, reason)
    }

    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        Forge::set_issue_label(self, repo, number, label, on)
    }
}
