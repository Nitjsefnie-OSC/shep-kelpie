//! Capabilities a role needs from its harness

use serde::Deserialize;

use super::Role;

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
