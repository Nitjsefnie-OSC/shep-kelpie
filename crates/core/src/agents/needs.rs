//! Capabilities a role needs from its harness

use serde::Deserialize;

use super::Role;
use crate::settings::Harness;

/// A capability an agent requires from its harness
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Need {
    /// Fenced edits to the worktree
    Edits,
    /// A session that continues across turns
    ResumableSession,
    /// Read access to the repository
    RepoReads,
    /// Kelpie's PM and issue-writer guards wired by the adapter
    RoleGuard,
}

impl Role {
    /// The role's default capabilities, which its file may override
    ///
    /// The PM and issue writer always require `RoleGuard`, even with an override.
    pub fn needs(self) -> &'static [Need] {
        match self {
            Self::Implementer => &[Need::Edits, Need::ResumableSession],
            Self::Reviewer | Self::Advisor | Self::Researcher | Self::Triager => &[Need::RepoReads],
            Self::IssueWriter | Self::Pm => &[Need::RoleGuard],
            Self::Representative => &[],
        }
    }
}

impl Need {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::Edits => "fenced edits",
            Self::ResumableSession => "a resumable session",
            Self::RepoReads => "repository reads",
            Self::RoleGuard => "kelpie's PM and issue-writer guards",
        }
    }

    pub(crate) fn met_by(self, harness: Harness) -> bool {
        let facts = harness.facts();
        match self {
            Self::Edits => facts.edits,
            Self::ResumableSession => facts.resumable_session,
            Self::RepoReads => facts.repo_reads,
            Self::RoleGuard => facts.role_guard,
        }
    }
}

impl Harness {
    /// Whether it provides every capability the role implies
    pub fn serves(self, role: Role) -> bool {
        role.needs().iter().all(|need| need.met_by(self))
    }

    /// The harnesses providing every capability the role implies, in `ALL` order
    pub fn serving(role: Role) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|harness| harness.serves(role))
            .collect()
    }
}
