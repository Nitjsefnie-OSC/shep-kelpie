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
    /// Claude Code hooks that hold the role's tools
    ClaudeHooks,
}

impl Role {
    /// The capabilities this role requires unless its file overrides them
    pub fn needs(self) -> &'static [Need] {
        match self {
            Self::Implementer => &[Need::Edits, Need::ResumableSession],
            Self::Reviewer | Self::Advisor | Self::Researcher | Self::Triager => &[Need::RepoReads],
            Self::IssueWriter | Self::Pm => &[Need::ClaudeHooks],
            Self::Representative => &[],
        }
    }
}

impl Need {
    pub(crate) fn met_by(self, harness: Harness) -> bool {
        let facts = harness.facts();
        match self {
            Self::Edits => facts.edits,
            Self::ResumableSession => facts.resumable_session,
            Self::RepoReads => facts.repo_reads,
            Self::ClaudeHooks => facts.claude_hooks,
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
