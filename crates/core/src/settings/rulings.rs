//! How rulings are decided, `[app.dogs.kelpie.rulings]`

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{Held, RulingKind, Stuck};

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
    /// The panel decides every ruling it votes on, and the maintainer is asked
    /// only when no expert voted. All four experts are asked at once, whatever
    /// the work item's type; the option with the most votes wins, a `design`
    /// flag does not stop it, and a tie goes to the project manager's choice,
    /// else the option listed first, though a tie never merges. `pm_alone` and
    /// `full_panel_paths` have no effect.
    Always,
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
    /// when absent, and `merge` and `security` are refused.
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
    /// A check stayed queued after its run was rerun
    CheckQueued,
    /// A head got no check
    NoChecks,
    /// The forge refused a merge twice
    MergeRefused,
    /// The pull request was closed
    Closed,
    /// A fix turn ended without a push
    FixNotPushed,
    /// A reviewer's test still fails after two fixes
    TestStillFails,
    /// Work in the worktree is not pushed
    Unpushed,
    /// A contributor's fork refused kelpie's push of the worker's commits
    PushRefused,
    /// A turn ran past its ceiling
    TurnTimeout,
    /// A turn failed
    TurnFailed,
    /// No implementer was picked
    Unlabelled,
    /// An adopted pull request could not start
    AdoptFailed,
    /// A work item or a contribution is over its budget
    OverBudget,
    /// The security review flagged a contribution's head. The panel never
    /// votes on it, so `pm_alone` refuses it.
    Security,
    /// A contribution's pass has ended and its CI is green on the head. The
    /// maintainer decides every merge of someone else's work, and the panel
    /// never votes on it, so `pm_alone` refuses it.
    ContributionMerge,
    /// The local model sat on the CPU. No ruling is raised for it now, and
    /// the name stays so ledger and history lines written before it went
    /// still read; `pm_alone` refuses it.
    LocalModelSpilled,
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
            RulingKind::Contribution(held) => match held {
                Held::Security { .. } => Self::Security,
                Held::OverBudget { .. } => Self::OverBudget,
                Held::Question { .. }
                | Held::Waiting
                | Held::NoEdits
                | Held::CannotAdopt { .. } => Self::Question,
                Held::Merge { .. } => Self::ContributionMerge,
            },
            RulingKind::Stuck(stuck) => match stuck {
                Stuck::Rebase { .. } => Self::Rebase,
                Stuck::StillRed { .. } => Self::StillRed,
                Stuck::CheckQueued { .. } => Self::CheckQueued,
                Stuck::NoChecks { .. } => Self::NoChecks,
                Stuck::MergeRefused { .. } => Self::MergeRefused,
                Stuck::Closed => Self::Closed,
                Stuck::FixNotPushed { .. } => Self::FixNotPushed,
                Stuck::TestStillFails { .. } => Self::TestStillFails,
                Stuck::Unpushed { .. } => Self::Unpushed,
                Stuck::PushRefused { .. } => Self::PushRefused,
                Stuck::TurnTimeout { .. } => Self::TurnTimeout,
                Stuck::TurnFailed { .. } => Self::TurnFailed,
                Stuck::Unlabelled { .. } => Self::Unlabelled,
                Stuck::AdoptFailed { .. } => Self::AdoptFailed,
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
            Self::CheckQueued => "check-queued",
            Self::NoChecks => "no-checks",
            Self::MergeRefused => "merge-refused",
            Self::Closed => "closed",
            Self::FixNotPushed => "fix-not-pushed",
            Self::TestStillFails => "test-still-fails",
            Self::Unpushed => "unpushed",
            Self::PushRefused => "push-refused",
            Self::TurnTimeout => "turn-timeout",
            Self::TurnFailed => "turn-failed",
            Self::Unlabelled => "unlabelled",
            Self::AdoptFailed => "adopt-failed",
            Self::OverBudget => "over-budget",
            Self::Security => "security",
            Self::ContributionMerge => "contribution-merge",
            Self::LocalModelSpilled => "local-model-spilled",
        }
    }
}

impl Rulings {
    /// Whether the project manager alone decides a ruling of `kind`
    pub fn pm_decides(&self, kind: RulingName) -> bool {
        self.pm_alone.contains(&kind)
    }

    /// Notes naming each of `pm_alone` and `full_panel_paths` that is set under
    /// `decide = "always"`, which ignores it
    pub fn unused_under_always(&self) -> Vec<String> {
        let set = [
            ("rulings.pm_alone", !self.pm_alone.is_empty()),
            (
                "rulings.full_panel_paths",
                !self.full_panel_paths.is_empty(),
            ),
        ];
        (set.into_iter())
            .filter(|(_, set)| *set && self.decide == Decide::Always)
            .map(|(name, _)| format!("rulings.decide is \"always\", so {name} has no effect"))
            .collect()
    }

    /// Why `pm_alone` is refused, if it is: a merge ruling is never decided
    /// by the project manager alone, nor is a contribution's, and a kind of
    /// ruling that is gone is not one to list
    pub fn refusal(&self) -> Option<String> {
        if self.pm_alone.contains(&RulingName::Merge) {
            return Some(
                "a merge ruling is never decided by the project manager alone: take `merge` \
                 out of the list"
                    .to_owned(),
            );
        }
        if self.pm_alone.contains(&RulingName::Security) {
            return Some(
                "a ruling on a contribution never goes to the panel, since text from a \
                 stranger must not reach a vote: take `security` out of the list"
                    .to_owned(),
            );
        }
        if self.pm_alone.contains(&RulingName::ContributionMerge) {
            return Some(
                "the maintainer decides every merge of someone else's work, and a contribution's \
                 ruling never goes to the panel: take `contribution-merge` out of the list"
                    .to_owned(),
            );
        }
        self.pm_alone
            .contains(&RulingName::LocalModelSpilled)
            .then(|| {
                "`local-model-spilled` is no longer a kind of ruling, since kelpie no longer \
                 checks where a local model sits: take it out of the list"
                    .to_owned()
            })
    }
}
