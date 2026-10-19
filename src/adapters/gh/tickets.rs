//! GitHub's issues as the ticket port (ADR 0014)
//!
//! A GitHub project's tickets are its issues, so this is where kelpie reads
//! and writes them: each method is the `gh` call for it, as the forge port
//! made it before ADR 0014 took the issue half off `Forge`. The impl stands
//! here rather than in `gh` itself, so no `gh` call site has both ports in
//! scope at once.

use super::{Gh, board, issue, issues, thread};
use crate::board::ReadyIssue;
use crate::ports::{
    FinishedParent, ForgeError, Issue, IssueCloseReason, OpenIssue, SubIssue, ThreadComment,
    Tickets,
};
use crate::settings::ForgeSlug;

impl Tickets for Gh {
    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError> {
        issue(None, repo, number)
    }

    fn sub_issues(&self, repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
        issues::sub_issues(repo, parent)
    }

    fn finished_parents(&self, repo: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError> {
        issues::finished_parents(repo)
    }

    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        board::ready_issues(None, repo)
    }

    fn open_issues(&self, repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
        issues::open_issues(repo)
    }

    fn issue_comments(
        &self,
        repo: &ForgeSlug,
        number: u64,
    ) -> Result<Vec<ThreadComment>, ForgeError> {
        thread::issue_comments(&self.etags, repo, number)
    }

    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        issues::create_issue(repo, title, body, labels)
    }

    fn close_issue(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        issues::close_issue(repo, number)
    }

    fn close_issue_as(
        &self,
        repo: &ForgeSlug,
        number: u64,
        reason: IssueCloseReason,
    ) -> Result<(), ForgeError> {
        issues::close_issue_as(repo, number, reason)
    }

    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        issues::set_label(repo, number, label, on)
    }
}
