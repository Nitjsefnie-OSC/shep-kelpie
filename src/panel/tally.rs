//! How the panel's votes decide a ruling
//!
//! The work item's type, from its pull request's conventional title, sets
//! the [`Tier`] the panel starts at, unless the ruling is a question or
//! touches a risky path, which start at the full panel (see
//! [`super::first_tier`]). A smaller panel decides only when everyone it asked
//! picked the same option and nobody flagged `design`; otherwise the next one
//! up is asked, or the full panel on a `design` flag. The full panel decides on
//! three votes of four. Anything less is for the maintainer. [`settle`] is one
//! stage of that, over the ballots so far, and knows nothing of the runner.

use serde::{Deserialize, Serialize};

use super::{Ballot, Vote, Voter};

/// How many of the four experts must pick one option to decide it
pub const MAJORITY: usize = 3;

/// A panel, by who it asks: what a type of work sets, and what is asked next
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// The project manager alone: `docs`
    Docs,
    /// The project manager and the advisor: `fix` and the types that change no behaviour
    Fix,
    /// All four: `feat`, a breaking change, or no type
    Full,
}

impl Tier {
    /// Every panel, smallest first
    pub const ALL: [Self; 3] = [Self::Docs, Self::Fix, Self::Full];

    /// Who it asks
    pub fn voters(self) -> &'static [Voter] {
        match self {
            Self::Docs => &[Voter::Pm],
            Self::Fix => &[Voter::Pm, Voter::Advisor],
            Self::Full => &Voter::ALL,
        }
    }

    /// The panel asked when this one does not decide, if there is one
    pub fn up(self) -> Option<Self> {
        match self {
            Self::Docs => Some(Self::Fix),
            Self::Fix => Some(Self::Full),
            Self::Full => None,
        }
    }

    /// What the usage ledger and `status` call it
    pub fn name(self) -> &'static str {
        match self {
            Self::Docs => "pm",
            Self::Fix => "pm+advisor",
            Self::Full => "all",
        }
    }
}

/// What a pull request's title says of its work item
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// Its type as the ledger and `status` split by it: the conventional
    /// type, `breaking` for one marked with `!`, or `untyped`
    pub kind: String,
    /// The panel its type asks
    pub tier: Tier,
}

/// The types that ask the project manager and the advisor
const FIX_TYPES: [&str; 7] = ["fix", "refactor", "perf", "test", "ci", "chore", "style"];

/// The work a pull request titled `title` is, or `untyped` with no title
///
/// A conventional title is `type(scope)!: summary`. `docs` asks the project
/// manager; `fix`, `refactor`, `perf`, `test`, `ci`, `chore` and `style` the
/// project manager and the advisor; `feat`, a type marked with `!`, any
/// other word and no type all four.
pub fn work_of(title: Option<&str>) -> Work {
    let untyped = Work {
        kind: "untyped".to_owned(),
        tier: Tier::Full,
    };
    let Some((head, _)) = title.and_then(|t| t.trim().split_once(':')) else {
        return untyped;
    };
    let (head, breaking) = match head.strip_suffix('!') {
        Some(head) => (head, true),
        None => (head, false),
    };
    let (kind, scope) = match head.split_once('(') {
        Some((kind, scope)) => (kind, Some(scope)),
        None => (head, None),
    };
    let scoped = scope.is_none_or(|scope| scope.ends_with(')'));
    let word = !kind.is_empty() && kind.chars().all(|c| c.is_ascii_lowercase());
    if !word || !scoped {
        return untyped;
    }
    let tier = match (kind, breaking) {
        (_, true) => Tier::Full,
        ("docs", false) => Tier::Docs,
        (kind, false) if FIX_TYPES.contains(&kind) => Tier::Fix,
        _ => Tier::Full,
    };
    let kind = match breaking {
        true => "breaking".to_owned(),
        false => kind.to_owned(),
    };
    Work { kind, tier }
}

/// Where the panel is asked: the project manager alone for a kind on
/// `rulings.pm_alone`, or a [`Tier`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The project manager decides with its own vote
    Alone,
    /// This panel
    Tier(Tier),
}

impl Stage {
    /// Who it asks
    pub fn voters(self) -> &'static [Voter] {
        match self {
            Self::Alone => &[Voter::Pm],
            Self::Tier(tier) => tier.voters(),
        }
    }

    /// What is asked when this stage does not decide: the panel the work's
    /// type sets after the project manager alone, the next one up after a panel
    pub fn up(self, work: Tier) -> Option<Self> {
        match self {
            Self::Alone => Some(Self::Tier(work)),
            Self::Tier(tier) => tier.up().map(Self::Tier),
        }
    }
}

/// The option a panel stands behind, and how many of those it asked chose it
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advice {
    /// The option's key
    pub choice: char,
    /// How many chose it
    pub agree: u32,
    /// How many were asked
    pub of: u32,
}

/// What the panel concluded about a ruling, kept on it
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    /// The work item's type, as [`Work::kind`] has it
    pub work: String,
    /// The panel that type set
    pub tier: Tier,
    /// What the panel stood behind, if anything
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advice: Option<Advice>,
    /// Whether the work item's diff touched a path on `rulings.full_panel_paths`
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub risky: bool,
    /// Whether all four experts voted blind and nothing was decided, as
    /// `rulings.decide = "shadow"` has it
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shadow: bool,
}

/// What one stage came to
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The panel decides on this
    Decided(Advice),
    /// It does not, and this stage is asked next
    Up(Stage),
    /// The full panel does not decide: the maintainer does
    Maintainer {
        /// What the panel stood behind, when three or more agreed
        advice: Option<Advice>,
        /// Whether an expert flagged `design`
        design: bool,
    },
}

/// What the votes of the experts a panel asks come to
pub(super) struct Judgement {
    /// The option they stand behind: all of a smaller panel on one option, or
    /// [`MAJORITY`] of the full one
    pub advice: Option<Advice>,
    /// Whether any of them flagged `design`
    pub design: bool,
}

/// What `asked` come to over `ballots`; an expert with no ballot abstains
pub(super) fn judge(asked: &[Voter], ballots: &[Ballot]) -> Judgement {
    let cast: Vec<Option<&Vote>> = (asked.iter())
        .map(|voter| {
            let ballot = ballots.iter().find(|b| b.voter == *voter);
            ballot.and_then(|b| b.vote.as_ref())
        })
        .collect();
    let design = cast.iter().flatten().any(|v| v.design);
    let of = u32::try_from(asked.len()).unwrap_or(u32::MAX);
    let advice = match asked.len() == Voter::ALL.len() {
        true => recommend(&cast, of),
        false => unanimous(&cast, of),
    };
    Judgement { advice, design }
}

/// What `stage` comes to over `ballots`; an expert with no ballot abstains
///
/// `work` is the panel asked after the project manager alone. A `design` flag in
/// a smaller panel asks the full one, whatever the work item's type.
pub fn settle(stage: Stage, work: Tier, ballots: &[Ballot]) -> Step {
    let Judgement { advice, design } = judge(stage.voters(), ballots);
    if stage == Stage::Tier(Tier::Full) {
        return match (advice, design) {
            (Some(advice), false) => Step::Decided(advice),
            (advice, design) => Step::Maintainer { advice, design },
        };
    }
    if let (Some(advice), false) = (advice, design) {
        return Step::Decided(advice);
    }
    let up = match design {
        true => Some(Stage::Tier(Tier::Full)),
        false => stage.up(work),
    };
    match up {
        Some(up) => Step::Up(up),
        None => Step::Maintainer {
            advice: None,
            design,
        },
    }
}

// The option every one of the votes chose, with `of` asked; an abstention is
// not agreement.
fn unanimous(cast: &[Option<&Vote>], of: u32) -> Option<Advice> {
    let choice = cast.first().copied().flatten().map(|v| v.choice)?;
    (cast.iter().all(|v| v.map(|v| v.choice) == Some(choice))).then_some(Advice {
        choice,
        agree: of,
        of,
    })
}

// The option at least [`MAJORITY`] of the votes chose, with `of` asked.
fn recommend(cast: &[Option<&Vote>], of: u32) -> Option<Advice> {
    let mut counts: Vec<(char, usize)> = Vec::new();
    for vote in cast.iter().flatten() {
        match counts.iter_mut().find(|(c, _)| *c == vote.choice) {
            Some((_, n)) => *n += 1,
            None => counts.push((vote.choice, 1)),
        }
    }
    let (choice, n) = counts.into_iter().max_by_key(|(_, n)| *n)?;
    (n >= MAJORITY).then_some(Advice {
        choice,
        agree: u32::try_from(n).unwrap_or(u32::MAX),
        of,
    })
}

/// What the panel comes to over `ballots` from `start` on, stage by stage as
/// it would be asked: the last stage, and what it came to
pub fn simulate(start: Stage, work: Tier, ballots: &[Ballot]) -> (Stage, Step) {
    let mut stage = start;
    loop {
        match settle(stage, work, ballots) {
            Step::Up(up) => stage = up,
            step => return (stage, step),
        }
    }
}

#[cfg(test)]
mod tests;
