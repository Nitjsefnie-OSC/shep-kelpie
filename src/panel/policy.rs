//! Who votes, as candidate policies scored against the votes of a shadow run
//!
//! Under `shadow` every expert votes blind on every ruling, so any policy for
//! who votes can be played over the ballots afterwards with no extra call: who
//! it would have asked, and what that came to. The candidates are fixed here
//! and are not a setting. Once the data picks one, a later change makes it
//! what `panel` means and removes the rest.

use super::tally::{judge, Judgement};
use super::{Ballot, Tier, Voter};

/// The panel a ruling starts at under `panel`: all four for a question or a
/// ruling whose diff touches a risky path, else the one `work`, its type's, sets
///
/// The work item's type is what its worker wrote in the title, so neither
/// of the other two facts can lower what it sets.
pub fn first_tier(work: Tier, question: bool, risky: bool) -> Tier {
    match question || risky {
        true => Tier::Full,
        false => work,
    }
}

/// A candidate policy for who votes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// The type's table alone: what `panel` was before the rules below
    Type,
    /// The type's table, with the full panel for a question, a `design` flag
    /// and a risky path: what `panel` is
    TypeRisk,
    /// The full panel for a question, a `design` flag and a risky path, the
    /// project manager and the advisor for every other ruling
    KindRisk,
    /// [`Policy::KindRisk`] with the researcher in the advisor's place
    KindRiskResearcher,
    /// The project manager and the advisor on every ruling, with nobody above
    PmAdvisor,
    /// The project manager and the researcher on every ruling, with nobody above
    PmResearcher,
    /// All four on every ruling
    All,
}

/// What a ruling is, for a policy to judge
#[derive(Debug, Clone, Copy)]
pub struct Case<'a> {
    /// Whether it is a worker's question
    pub question: bool,
    /// Whether its diff touches a path on `rulings.full_panel_paths`
    pub risky: bool,
    /// The panel its work item's type sets
    pub tier: Tier,
    /// The votes of a shadow run, which has every expert vote
    pub ballots: &'a [Ballot],
}

/// What a policy comes to on a ruling
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Everyone it would have asked, escalations included
    pub asked: Vec<Voter>,
    /// The option it would have decided on, or none when the maintainer is asked
    pub decided: Option<char>,
}

// One panel of a policy's ladder.
type Rung = &'static [Voter];

const PM_RESEARCHER: Rung = &[Voter::Pm, Voter::Researcher];

impl Policy {
    /// Every candidate, in the order `status` lists them
    pub const ALL: [Self; 7] = [
        Self::Type,
        Self::TypeRisk,
        Self::KindRisk,
        Self::KindRiskResearcher,
        Self::PmAdvisor,
        Self::PmResearcher,
        Self::All,
    ];

    /// What `status` calls it
    pub fn name(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::TypeRisk => "type+risk",
            Self::KindRisk => "kind+risk",
            Self::KindRiskResearcher => "kind+risk-researcher",
            Self::PmAdvisor => "pm+advisor",
            Self::PmResearcher => "pm+researcher",
            Self::All => "all",
        }
    }

    /// Who it would have asked on `case`, and what they came to
    pub fn play(self, case: &Case<'_>) -> Outcome {
        let risk = case.question || case.risky;
        let full = Tier::Full.voters();
        let ladder: Vec<Rung> = match self {
            Self::Type => from(case.tier),
            Self::TypeRisk => from(first_tier(case.tier, case.question, case.risky)),
            Self::KindRisk => from(first_tier(Tier::Fix, risk, false)),
            Self::KindRiskResearcher => match risk {
                true => vec![full],
                false => vec![PM_RESEARCHER, full],
            },
            Self::PmAdvisor => vec![Tier::Fix.voters()],
            Self::PmResearcher => vec![PM_RESEARCHER],
            Self::All => vec![full],
        };
        walk(&ladder, case.ballots)
    }
}

// The panels from `tier` up, each asked when the one before does not decide.
fn from(tier: Tier) -> Vec<Rung> {
    let mut ladder = vec![tier.voters()];
    let mut at = tier;
    while let Some(up) = at.up() {
        ladder.push(up.voters());
        at = up;
    }
    ladder
}

// Asks each panel of `ladder` in turn until one decides. A panel that does not
// decide asks the next, or the last on a `design` flag; the last one that does
// not decide leaves the ruling to the maintainer.
fn walk(ladder: &[Rung], ballots: &[Ballot]) -> Outcome {
    let mut asked: Vec<Voter> = Vec::new();
    let mut at = 0;
    loop {
        let rung = ladder[at];
        for voter in rung {
            if !asked.contains(voter) {
                asked.push(*voter);
            }
        }
        let Judgement { advice, design } = judge(rung, ballots);
        if let (Some(advice), false) = (advice, design) {
            return Outcome {
                asked,
                decided: Some(advice.choice),
            };
        }
        let last = ladder.len() - 1;
        if at == last {
            return Outcome {
                asked,
                decided: None,
            };
        }
        at = match design {
            true => last,
            false => at + 1,
        };
    }
}

#[cfg(test)]
mod tests;
