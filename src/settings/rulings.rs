//! How rulings are decided, `[app.dogs.kelpie.rulings]`

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{RulingKind, Stuck};

/// Whether the panel decides a ruling or only votes on it
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Decide {
    /// The panel votes and posts on every ruling, and the maintainer still
    /// answers it. Kelpie counts how often the panel's recommendation matched.
    #[default]
    Shadow,
    /// The panel decides a ruling when three experts agree on one option and
    /// none flagged `design`, or a smaller panel agrees, as the work item's
    /// type sets. Anything else goes to the maintainer.
    Panel,
}

/// How rulings are decided
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rulings {
    /// Whether the panel decides a ruling. `shadow` when absent.
    #[serde(default)]
    pub decide: Decide,
    /// Kinds of ruling the project manager decides with its own vote on any
    /// type of work, when it flags no `design` and does not abstain. Empty
    /// when absent, and `merge` is refused.
    #[serde(default)]
    pub pm_alone: Vec<RulingName>,
    /// Globs of paths that call the full panel for a ruling on a work item
    /// whose diff from its merge base touches one, whatever its type. `*` and
    /// `?` stay inside a folder and `**` spans folders. Empty when absent.
    #[serde(default)]
    pub full_panel_paths: Vec<String>,
}

/// A kind of ruling, as `rulings.pm_alone` and the usage ledger name it
// wire format: changing this is a breaking change to the rulings file
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum RulingName {
    /// A merge: CI is green on a head and a yes merges it
    Merge,
    /// The worker's question
    Question,
    /// A pull request changes agents' own files
    AgentFiles,
    /// A pull request's labels or ready state changed outside kelpie
    ForeignChange,
    /// A merged pull request left findings unfixed
    FollowUp,
    /// The branch could not be rebased
    Rebase,
    /// CI is still red
    StillRed,
    /// The forge refused a merge twice
    MergeRefused,
    /// The pull request was closed
    Closed,
    /// The local model spilled off the GPU
    LocalModelSpilled,
    /// A fix turn ended without a push
    FixNotPushed,
    /// Work in the worktree is not pushed
    Unpushed,
    /// A turn ran past its ceiling
    TurnTimeout,
    /// A turn failed
    TurnFailed,
    /// No implementer was picked
    Unlabelled,
    /// A work item is over its budget
    OverBudget,
}

impl RulingName {
    /// The kind of `kind`
    pub fn of(kind: &RulingKind) -> Self {
        match kind {
            RulingKind::Merge { .. } => Self::Merge,
            RulingKind::Question { .. } => Self::Question,
            RulingKind::AgentFiles { .. } => Self::AgentFiles,
            RulingKind::ForeignChange { .. } => Self::ForeignChange,
            RulingKind::FollowUp { .. } => Self::FollowUp,
            RulingKind::Stuck(stuck) => match stuck {
                Stuck::Rebase { .. } => Self::Rebase,
                Stuck::StillRed { .. } => Self::StillRed,
                Stuck::MergeRefused { .. } => Self::MergeRefused,
                Stuck::Closed => Self::Closed,
                Stuck::LocalModelSpilled { .. } => Self::LocalModelSpilled,
                Stuck::FixNotPushed { .. } => Self::FixNotPushed,
                Stuck::Unpushed { .. } => Self::Unpushed,
                Stuck::TurnTimeout { .. } => Self::TurnTimeout,
                Stuck::TurnFailed { .. } => Self::TurnFailed,
                Stuck::Unlabelled { .. } => Self::Unlabelled,
                Stuck::OverBudget { .. } => Self::OverBudget,
            },
        }
    }

    /// The name as the settings and the ledger spell it
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Question => "question",
            Self::AgentFiles => "agent-files",
            Self::ForeignChange => "foreign-change",
            Self::FollowUp => "follow-up",
            Self::Rebase => "rebase",
            Self::StillRed => "still-red",
            Self::MergeRefused => "merge-refused",
            Self::Closed => "closed",
            Self::LocalModelSpilled => "local-model-spilled",
            Self::FixNotPushed => "fix-not-pushed",
            Self::Unpushed => "unpushed",
            Self::TurnTimeout => "turn-timeout",
            Self::TurnFailed => "turn-failed",
            Self::Unlabelled => "unlabelled",
            Self::OverBudget => "over-budget",
        }
    }
}

impl Rulings {
    /// Whether the project manager alone decides a ruling of `kind`
    pub fn pm_decides(&self, kind: RulingName) -> bool {
        self.pm_alone.contains(&kind)
    }

    /// Why `pm_alone` is refused, if it is: a merge ruling is never decided
    /// by the project manager alone
    pub fn refusal(&self) -> Option<String> {
        self.pm_alone.contains(&RulingName::Merge).then(|| {
            "a merge ruling is never decided by the project manager alone: take `merge` \
             out of the list"
                .to_owned()
        })
    }
}
