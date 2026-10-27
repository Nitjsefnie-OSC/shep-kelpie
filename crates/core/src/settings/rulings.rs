//! How rulings are decided, `[app.dogs.kelpie.rulings]`

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::SettingsError;
use crate::state::{Held, RulingKind, Stuck};

/// The setting that says which kinds of ruling the project manager decides
pub const PM_DECIDES: &str = "rulings.pm_decides";

/// The setting that lists kinds the project manager decides, each read as
/// `true` in `rulings.pm_decides`
pub const PM_ALONE: &str = "rulings.pm_alone";

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
    /// else the option listed first, though a tie never merges. `pm_decides`
    /// and `full_panel_paths` have no effect.
    Always,
}

/// How rulings are decided
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "Written")]
pub struct Rulings {
    /// Whether the panel decides a ruling. `shadow` when absent.
    pub decide: Decide,
    /// Whether the project manager decides each kind of ruling, `true`, or
    /// the maintainer does. A kind it does not name is the maintainer's.
    pub pm_decides: BTreeMap<RulingName, bool>,
    /// Globs of paths that call the full panel for a ruling on a work item
    /// whose diff from its merge base touches one, whatever its type.
    pub full_panel_paths: Vec<String>,
    // Whether `pm_decides` was read from the old list, for the lines that name the key
    #[schemars(skip)]
    from_list: bool,
}

/// How rulings are decided
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Written {
    /// Whether the panel decides a ruling. `shadow` when absent.
    #[serde(default)]
    decide: Decide,
    /// Whether the project manager decides each kind of ruling with its own
    /// vote on any type of work, `true`, when it flags no `design` and does
    /// not abstain, or the maintainer does, `false`. A kind it does not name
    /// is the maintainer's. `merge`, `security`, `contribution-merge` and
    /// `local-model-spilled` are refused as `true`.
    #[serde(default)]
    #[schemars(with = "BTreeMap<RulingName, bool>")]
    pm_decides: Option<BTreeMap<RulingName, bool>>,
    /// Kinds the project manager decides alone, each read as `true` in
    /// `pm_decides`. Refused beside `pm_decides`.
    #[serde(default)]
    #[schemars(skip)]
    pm_alone: Option<Vec<RulingName>>,
    /// Globs of paths that call the full panel for a ruling on a work item
    /// whose diff from its merge base touches one, whatever its type. `*` and
    /// `?` stay inside a folder and `**` spans folders. Empty when absent.
    #[serde(default)]
    full_panel_paths: Vec<String>,
}

impl TryFrom<Written> for Rulings {
    type Error = String;

    fn try_from(written: Written) -> Result<Self, String> {
        let (pm_decides, from_list) = match (written.pm_decides, written.pm_alone) {
            (Some(_), Some(_)) => {
                return Err(format!(
                    "set `{PM_DECIDES}` or the old `{PM_ALONE}`, not both: move the list's \
                     kinds into the map as `true`"
                ));
            }
            (Some(map), None) => (map, false),
            (None, Some(list)) => (list.into_iter().map(|kind| (kind, true)).collect(), true),
            (None, None) => (BTreeMap::new(), false),
        };
        Ok(Self {
            decide: written.decide,
            pm_decides,
            full_panel_paths: written.full_panel_paths,
            from_list,
        })
    }
}

/// A kind of ruling, as `rulings.pm_decides` and the usage ledger name it
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
    /// votes on it, so `pm_decides` refuses it.
    Security,
    /// A contribution's pass has ended and its CI is green on the head. The
    /// maintainer decides every merge of someone else's work, and the panel
    /// never votes on it, so `pm_decides` refuses it.
    ContributionMerge,
    /// The local model sat on the CPU. No ruling is raised for it now, and
    /// the name stays so ledger and history lines written before it went
    /// still read; `pm_decides` refuses it.
    LocalModelSpilled,
}

impl RulingName {
    /// Every kind, in the order the enum lists them
    pub const ALL: [Self; 23] = [
        Self::Merge,
        Self::Question,
        Self::AgentFiles,
        Self::ForeignChange,
        Self::FollowUp,
        Self::Rebase,
        Self::StillRed,
        Self::CheckQueued,
        Self::NoChecks,
        Self::MergeRefused,
        Self::Closed,
        Self::FixNotPushed,
        Self::TestStillFails,
        Self::Unpushed,
        Self::PushRefused,
        Self::TurnTimeout,
        Self::TurnFailed,
        Self::Unlabelled,
        Self::AdoptFailed,
        Self::OverBudget,
        Self::Security,
        Self::ContributionMerge,
        Self::LocalModelSpilled,
    ];

    /// What `rulings.pm_decides` starts this kind at, or none for a kind
    /// the project manager is refused and that is never written
    pub fn pm_default(self) -> Option<bool> {
        match self {
            Self::Merge | Self::Security | Self::ContributionMerge | Self::LocalModelSpilled => {
                None
            }
            Self::Question | Self::ForeignChange | Self::AgentFiles => Some(false),
            Self::FollowUp
            | Self::Rebase
            | Self::StillRed
            | Self::CheckQueued
            | Self::NoChecks
            | Self::MergeRefused
            | Self::Closed
            | Self::FixNotPushed
            | Self::TestStillFails
            | Self::Unpushed
            | Self::PushRefused
            | Self::TurnTimeout
            | Self::TurnFailed
            | Self::Unlabelled
            | Self::AdoptFailed
            | Self::OverBudget => Some(true),
        }
    }

    // Why the project manager never decides this kind, for the kinds it never does.
    fn refused_because(self) -> Option<&'static str> {
        match self {
            Self::Merge => Some("a merge ruling is never decided by the project manager alone"),
            Self::Security => Some(
                "a ruling on a contribution never goes to the panel, since text from a stranger \
                 must not reach a vote",
            ),
            Self::ContributionMerge => Some(
                "the maintainer decides every merge of someone else's work, and a contribution's \
                 ruling never goes to the panel",
            ),
            Self::LocalModelSpilled => Some(
                "`local-model-spilled` is no longer a kind of ruling, since kelpie no longer \
                 checks where a local model sits",
            ),
            _ => None,
        }
    }

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
        self.pm_decides.get(&kind) == Some(&true)
    }

    // The key the kinds were read from, as the lines that name it spell it.
    fn key(&self) -> &'static str {
        match self.from_list {
            true => PM_ALONE,
            false => PM_DECIDES,
        }
    }

    /// Notes naming each of `pm_decides` and `full_panel_paths` that is set
    /// under `decide = "always"`, which ignores it
    pub fn unused_under_always(&self) -> Vec<String> {
        let set = [
            (self.key(), self.pm_decides.values().any(|pm| *pm)),
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

    /// Why the kinds are refused, if they are, naming the key they were read
    /// from: a kind the project manager never decides is set to `true`
    pub fn refusal(&self) -> Option<SettingsError> {
        let (kind, because) = (RulingName::ALL.into_iter())
            .filter(|kind| self.pm_decides(*kind))
            .find_map(|kind| Some((kind, kind.refused_because()?)))?;
        let fix = match self.from_list {
            true => "out of the list",
            false => "out of the map, or set it to false",
        };
        Some(SettingsError::Invalid {
            setting: self.key(),
            reason: format!("{because}: take `{}` {fix}", kind.as_str()),
        })
    }
}
