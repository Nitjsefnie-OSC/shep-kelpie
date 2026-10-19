//! The tickets a project's work items come from, as their source holds them
//!
//! ADR 0014: a ticket lives in exactly one source, and that source is its
//! truth. Kelpie reads a ticket and writes to it through this port. Every
//! project's source today is GitHub issues in the pull request's own repo,
//! where each method is the read or write the [`Forge`] already has. A local
//! store in kelpie's home, and ClickUp, follow as other sources.
//!
//! The board stays kelpie's whichever source the tickets come from: the saved
//! order, the project manager's holds and picks, drops and the file-overlap
//! check are kelpie's scheduling, not the source's.

#[cfg(doc)]
use super::Forge;
use super::{
    FinishedParent, ForgeError, Issue, IssueCloseReason, OpenIssue, SubIssue, ThreadComment,
};
#[cfg(doc)]
use crate::board::READY;
use crate::board::ReadyIssue;
use crate::settings::ForgeSlug;

/// The tickets of one project, as their source holds them
///
/// A ticket's id is opaque to kelpie: whatever its source gives it. A GitHub
/// ticket's id is its issue number, so `number` is an issue number wherever
/// the tickets are GitHub issues.
pub trait Tickets: Send + Sync {
    /// Issue `number` on `repo`
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked, has no such issue, or
    /// its answer cannot be read.
    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError>;

    /// The sub-issues of issue `parent` on `repo`, with how each stands
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked or its answer cannot be read.
    fn sub_issues(&self, repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError>;

    /// The open issues on `repo`, ready or not, that have sub-issues and whose
    /// sub-issues are all closed, whatever they closed as
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked or its answer read.
    fn finished_parents(&self, repo: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError>;

    /// The open issues on `repo` labelled [`READY`]
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked or its answer read.
    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError>;

    /// The open issues on `repo`, for telling a finding already filed
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked or its answer read.
    fn open_issues(&self, repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError>;

    /// Every comment on issue `number` of `repo`, oldest first
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge cannot be asked or its answer read.
    fn issue_comments(
        &self,
        repo: &ForgeSlug,
        number: u64,
    ) -> Result<Vec<ThreadComment>, ForgeError>;

    /// Opens an issue on `repo` with these labels, and returns its number
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge refuses, or its answer names no number.
    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError>;

    /// Closes issue `number`, with no comment: a comment would be posted as the
    /// login `gh` acts as, and only [`Forge::comment`] records the id of one
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge refuses or cannot be asked.
    fn close_issue(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError>;

    /// Closes issue `number` for `reason`, which the forge records on it
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge refuses or cannot be asked.
    fn close_issue_as(
        &self,
        repo: &ForgeSlug,
        number: u64,
        reason: IssueCloseReason,
    ) -> Result<(), ForgeError>;

    /// Adds `label` to issue `number`, or takes it off
    ///
    /// # Errors
    ///
    /// [`ForgeError`] when the forge refuses or cannot be asked.
    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError>;
}
