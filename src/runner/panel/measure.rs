//! What the maintainer's answers say of the panel
//!
//! Each ruling the maintainer answers is a line in `rulings.jsonl` with the
//! panel's ballots and what the panel concluded, so `status` can say from it
//! how the panel stands: the run of rulings its recommendation matched, the
//! same for the project manager's vote alone by kind of ruling, each expert's
//! agreement and how often it alone was right (also by kind of ruling), what
//! each smaller panel would have decided against the answers, and what each
//! candidate policy for who votes would have done over the rulings answered
//! under `shadow`. The panel's cost is in the usage ledger, `shep kelpie usage`
//! reads it there, and so does the cost of a policy.

use std::collections::BTreeMap;

use serde::Serialize;

use super::history::Record;
use crate::panel::{Case, Policy, Skip, Stage, Step, Tier, Voter, settle};
use crate::settings::{Decide, RulingName};

/// After this many rulings the panel has voted on, `docs/facts/the-panel.md`
/// is due an entry on what it cost and how it did
pub(super) const FACTS_AFTER: usize = 30;

/// How the panel stands, as `status` reports it
#[derive(Debug, Default, PartialEq, Serialize)]
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
    /// What each candidate policy for who votes would have done over the
    /// rulings answered under `shadow`, by its name
    pub whatif: BTreeMap<String, Score>,
    /// Whether `docs/facts/the-panel.md` is due its entry on the panel
    pub facts_due: bool,
    /// Rulings answered that the panel cast no vote on, by the reason
    pub skipped: BTreeMap<String, usize>,
}

/// One expert's votes against the maintainer's answers
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Expert {
    /// Votes it gave on rulings the maintainer answered
    pub votes: u32,
    /// Times it abstained
    pub abstained: u32,
    /// Votes that were the maintainer's answer
    pub agreed: u32,
    /// Times it was the only one whose vote was the maintainer's answer
    pub lone: u32,
    /// `agreed` and `lone` by kind of ruling, over the same answers
    pub by_kind: BTreeMap<String, Agreement>,
}

/// How often an expert agreed with the maintainer on one kind of ruling
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Agreement {
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

/// What a policy for who votes would have done over the rulings answered under `shadow`
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
pub struct Score {
    /// Rulings it would have decided without the maintainer
    pub decided: u32,
    /// Of those, the ones it decided as the maintainer answered
    pub matched: u32,
    /// Rulings it would have sent to the maintainer
    pub maintainer: u32,
    /// What it would have cost a ruling, in US dollars: the ledger lines of
    /// the votes it asks, its escalations included. A call with no reported
    /// cost adds nothing. None when the
    /// ledger could not be read.
    pub cost: Option<f64>,
}

/// How the panel stands over `records`, the rulings answered in order, with
/// what each expert's vote on each ruling cost in `costs`
pub(super) fn standing(
    decide: Decide,
    records: &[Record],
    costs: Option<&BTreeMap<(u64, Voter), f64>>,
) -> Standing {
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
    for policy in Policy::ALL {
        let score = Score {
            cost: costs.map(|_| 0.0),
            ..Score::default()
        };
        stand.whatif.insert(policy.name().to_owned(), score);
    }
    for skip in [Skip::Draining, Skip::Posted] {
        stand.skipped.insert(skip.name().to_owned(), 0);
    }
    for skip in records.iter().filter_map(|r| r.skipped) {
        *stand.skipped.entry(skip.name().to_owned()).or_default() += 1;
    }
    let mut scored: u32 = 0;
    for record in records.iter().filter(|r| !r.ballots.is_empty()) {
        stand.voted += 1;
        stand.decided += usize::from(record.by_panel);
        if record.by_maintainer() {
            stand.answered += 1;
            count(&mut stand, record);
            if let Some(case) = shadowed(record) {
                scored += 1;
                play(&mut stand, record.id, record.chose, &case, costs);
            }
        }
    }
    if scored > 0 {
        for score in stand.whatif.values_mut() {
            score.cost = score.cost.map(|cost| cost / f64::from(scored));
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
        let by_kind = record
            .kind
            .map(|kind| expert.by_kind.entry(kind.as_str().to_owned()));
        match &ballot.vote {
            None => expert.abstained += 1,
            Some(vote) => {
                expert.votes += 1;
                let agreed = Some(vote.choice) == chose;
                let lone = agreed && agreeing == [ballot.voter];
                expert.agreed += u32::from(agreed);
                expert.lone += u32::from(lone);
                if let Some(by_kind) = by_kind {
                    let by_kind = by_kind.or_default();
                    by_kind.agreed += u32::from(agreed);
                    by_kind.lone += u32::from(lone);
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

// The case a ruling answered under `shadow` makes for a policy, if it was.
fn shadowed(record: &Record) -> Option<Case<'_>> {
    let verdict = record.verdict.as_ref().filter(|v| v.shadow)?;
    Some(Case {
        question: record.kind == Some(RulingName::Question),
        risky: verdict.risky,
        tier: verdict.tier,
        ballots: &record.ballots,
    })
}

// Scores every policy on ruling `id`, answered as `chose`.
fn play(
    stand: &mut Standing,
    id: u64,
    chose: Option<char>,
    case: &Case<'_>,
    costs: Option<&BTreeMap<(u64, Voter), f64>>,
) {
    for policy in Policy::ALL {
        let outcome = policy.play(case);
        let score = stand.whatif.entry(policy.name().to_owned()).or_default();
        match outcome.decided {
            Some(choice) => {
                score.decided += 1;
                score.matched += u32::from(Some(choice) == chose);
            }
            None => score.maintainer += 1,
        }
        if let (Some(total), Some(costs)) = (&mut score.cost, costs) {
            let asked = outcome.asked.iter();
            *total +=
                (asked.map(|voter| costs.get(&(id, *voter)).copied().unwrap_or(0.0))).sum::<f64>();
        }
    }
}

#[cfg(test)]
mod tests;
