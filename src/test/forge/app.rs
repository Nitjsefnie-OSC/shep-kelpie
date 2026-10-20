//! The fake forge standing in for the App's reads

use std::path::Path;

use super::FakeForge;
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::github::{Apps, Owner};
use crate::ports::{
    AppForges, AppReads, Forge, ForgeError, Issue, PullRequest, Reading, Reviewed, Tickets,
};
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

/// The App registered in a kelpie home, which reads nothing: the login reads every repo, and the
/// App's login is the registered one's
#[derive(Debug, Clone)]
pub(crate) struct RegisteredApp(Apps);

impl RegisteredApp {
    /// The Apps kept in `kelpie_home`
    pub(crate) fn under(kelpie_home: &Path) -> Self {
        Self(Apps::under(kelpie_home))
    }
}

impl AppForges for RegisteredApp {
    fn reading(&self, _: &ForgeSlug) -> Reading {
        Reading::Login
    }

    fn login(&self, repo: &ForgeSlug) -> Option<String> {
        let owner = Owner::of(repo).ok()?;
        Some(self.0.get(&owner).ok()??.slug)
    }
}
