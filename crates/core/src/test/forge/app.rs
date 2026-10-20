//! The fake forge standing in for the App's reads

use super::FakeForge;
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::ports::{AppReads, Forge, ForgeError, Issue, PullRequest, Reviewed, Tickets};
use crate::review_bot::{Activity, Login};
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;

/// A second `FakeForge` is the App's: it answers the reads as the first does
/// and counts its own calls, so a test sees which of the two was asked
impl AppReads for FakeForge {
    fn issue(&self, repo: &ForgeSlug, ticket: TicketId) -> Result<Issue, ForgeError> {
        Tickets::issue(self, repo, ticket)
    }

    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        Tickets::ready_issues(self, repo)
    }

    fn open_pull_requests(&self, repo: &ForgeSlug) -> Result<Vec<OpenPullRequest>, ForgeError> {
        Forge::open_pull_requests(self, repo)
    }

    fn pull_request(&self, repo: &ForgeSlug, number: u64) -> Result<PullRequest, ForgeError> {
        Forge::pull_request(self, repo, number)
    }

    fn reviewed(&self, repo: &ForgeSlug, number: u64) -> Result<Reviewed, ForgeError> {
        Forge::reviewed(self, repo, number)
    }

    fn review_bot(
        &self,
        repo: &ForgeSlug,
        number: u64,
        login: Login<'_>,
    ) -> Result<Activity, ForgeError> {
        Forge::review_bot(self, repo, number, login)
    }
}
