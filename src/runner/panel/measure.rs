//! What the maintainer's answers say of the panel
//!
//! Each ruling the maintainer answers is a line in `rulings.jsonl` with the
//! panel's ballots and what the panel concluded, so `status` can say from it
//! how the panel stands: the run of rulings its recommendation matched, the
//! same for the project manager's vote alone by kind of ruling, each expert's
//! agreement and how often it alone was right, and what each smaller panel
//! would have decided against the answers. The panel's cost is in the usage
//! ledger, and `shep kelpie usage` reads it there.

use std::collections::BTreeMap;

use serde::Serialize;

use super::history::Record;
use crate::panel::{Stage, Step, Tier, Voter, settle};
use crate::settings::Decide;

/// After this many rulings the panel has voted on, the design log is due a
/// Facts entry on what it cost and how it did
pub(super) const FACTS_AFTER: usize = 30;

/// How the panel stands, as `status` reports it
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Standing {
    /// Whether the panel decides or only votes
    pub decide: Decide,
    /// Rulings the panel has voted on, whoever answered them
    pub voted: usize,
    /// Rulings the panel decided
    pub decided: usize,
    /// Rulings the panel voted on that the maintainer answered, which the rest count
    pub answered: usize,
    /// The run of those, latest last, where the panel's recommendation was
    /// the maintainer's answer. A ruling with no recommendation leaves the
    /// run as it is.
    pub streak: u32,
    /// For each kind of ruling, the run where the project manager's own
    /// vote was the maintainer's answer. A vote that flagged `design` or
    /// abstained leaves it as it is.
    pub pm_alone: BTreeMap<String, u32>,
    /// Each expert's votes against the maintainer's answers
    pub experts: BTreeMap<String, Expert>,
    /// What each panel, asked alone, would have decided against the answers
    pub panels: BTreeMap<String, Panel>,
    /// Whether the design log is due its Facts entry on the panel
    pub facts_due: bool,
}

/// One expert's votes against the maintainer's answers
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Expert {
    /// Votes it gave on rulings the maintainer answered
    pub votes: u32,
    /// Times it abstained
    pub abstained: u32,
    /// Votes that were the maintainer's answer
    pub agreed: u32,
    /// Times it was the only one whose vote was the maintainer's answer
    pub lone: u32,
}

/// What a panel would have decided, asked alone, against the answers
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Panel {
    /// Rulings it would have decided
    pub decided: u32,
    /// Of those, the ones it decided as the maintainer answered
    pub matched: u32,
}

/// How the panel stands over `records`, the rulings answered in order
pub(super) fn standing(decide: Decide, records: &[Record]) -> Standing {
    let mut stand = Standing {
        decide,
        ..Standing::default()
    };
    for voter in Voter::ALL {
        stand
            .experts
            .insert(voter.name().to_lowercase(), Expert::default());
    }
    for tier in Tier::ALL {
        stand
            .panels
            .insert(tier.name().to_owned(), Panel::default());
    }
    for record in records.iter().filter(|r| !r.ballots.is_empty()) {
        stand.voted += 1;
        stand.decided += usize::from(record.by_panel);
        if record.by_maintainer() {
            stand.answered += 1;
            count(&mut stand, record);
        }
    }
    stand.facts_due = stand.voted >= FACTS_AFTER;
    stand
}

// Counts one ruling the maintainer answered.
fn count(stand: &mut Standing, record: &Record) {
    let chose = record.chose;
    let advice = record.verdict.as_ref().and_then(|v| v.advice);
    if let Some(advice) = advice {
        stand.streak = match Some(advice.choice) == chose {
            true => stand.streak.saturating_add(1),
            false => 0,
        };
    }
    // The project manager's own vote, by kind.
    let pm = (record.ballots.iter())
        .find(|b| b.voter == Voter::Pm)
        .and_then(|b| b.vote.as_ref());
    if let (Some(vote), Some(kind)) = (pm, record.kind)
        && !vote.design
    {
        let run = stand.pm_alone.entry(kind.as_str().to_owned()).or_default();
        *run = match Some(vote.choice) == chose {
            true => run.saturating_add(1),
            false => 0,
        };
    }
    // Each expert, and the one right alone.
    let agreeing: Vec<Voter> = (record.ballots.iter())
        .filter(|b| matches!((&b.vote, chose), (Some(v), Some(c)) if v.choice == c))
        .map(|b| b.voter)
        .collect();
    for ballot in &record.ballots {
        let expert = stand
            .experts
            .entry(ballot.voter.name().to_lowercase())
            .or_default();
        match &ballot.vote {
            None => expert.abstained += 1,
            Some(vote) => {
                expert.votes += 1;
                if Some(vote.choice) == chose {
                    expert.agreed += 1;
                    expert.lone += u32::from(agreeing == [ballot.voter]);
                }
            }
        }
    }
    // What each panel would have decided, asked alone.
    for tier in Tier::ALL {
        if let Step::Decided(advice) = settle(Stage::Tier(tier), tier, &record.ballots) {
            let panel = stand.panels.entry(tier.name().to_owned()).or_default();
            panel.decided += 1;
            panel.matched += u32::from(Some(advice.choice) == chose);
        }
    }
}

#[cfg(test)]
mod tests;
