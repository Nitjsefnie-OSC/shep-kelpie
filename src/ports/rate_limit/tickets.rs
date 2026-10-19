//! The rate-limit hold as the ticket port (ADR 0014)
//!
//! A GitHub project's tickets are its issues, and every read and write of one
//! is held, paced and ledged as the same call through the forge is. The impl
//! stands here rather than in `rate_limit` itself, so no call site there has
//! both ports in scope at once.

use super::RateHeld;
use crate::board::ReadyIssue;
use crate::ports::{
    FinishedParent, Forge, ForgeError, Issue, IssueCloseReason, OpenIssue, SubIssue, ThreadComment,
    Tickets,
};
use crate::settings::ForgeSlug;

impl Tickets for RateHeld {
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
