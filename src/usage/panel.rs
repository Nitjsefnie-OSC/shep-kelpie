//! The panel's votes in `shep kelpie usage`: what they cost, split by expert,
//! by kind of ruling and by the work item's type
//!
//! Each vote is a [`VoteMark`] on the line of the call that cast it. An
//! expert's call has one; the project manager's wake has one for each
//! ruling it voted on, and shares its cost and counts its whole wall time
//! for each. A wake that also served another reason counts in full.

use std::collections::{BTreeMap, BTreeSet};

use super::{CallLine, Line, VoteMark};
use crate::panel::Voter;
use crate::ticket::TicketId;

// How a vote is grouped.
type Key = fn(&VoteMark) -> String;

/// What one group of votes cost and did
#[derive(Debug, Default)]
struct Share {
    calls: usize,
    cost: f64,
    unpriced: usize,
    seconds: Vec<f64>,
    abstained: usize,
    rulings: BTreeSet<TicketId>,
}

/// What each expert's vote on each ruling cost, in US dollars, from the call
/// lines that carry its mark
///
/// A call that cast several votes shares its cost between them. A call with
/// no reported cost adds nothing, so a vote with none is missing from the map.
pub fn vote_costs(lines: &[Line]) -> BTreeMap<(TicketId, Voter), f64> {
    let mut costs = BTreeMap::new();
    for line in lines {
        let Line::Call(call) = line else { continue };
        let Some(cost) = call.cost_usd else { continue };
        let share = cost / call.votes.len().max(1) as f64;
        for vote in &call.votes {
            *costs.entry((vote.ruling, vote.voter)).or_insert(0.0) += share;
        }
    }
    costs
}

/// The lines that split the panel's votes, if any call cast one
pub(super) fn split(calls: &[&CallLine]) -> Vec<String> {
    let voted: Vec<&&CallLine> = calls.iter().filter(|c| !c.votes.is_empty()).collect();
    if voted.is_empty() {
        return Vec::new();
    }
    let rulings: BTreeSet<TicketId> = (voted.iter())
        .flat_map(|c| c.votes.iter().map(|v| v.ruling))
        .collect();
    let cost: f64 = voted.iter().filter_map(|c| c.cost_usd).sum();
    let mut out = vec![format!(
        "panel votes on {}, {} a ruling, split by:",
        count(rulings.len(), "ruling"),
        per(cost, rulings.len())
    )];
    let by: [(&str, Key); 3] = [
        ("expert", |v| v.voter.name().to_lowercase()),
        ("kind", |v| v.ruling_kind.as_str().to_owned()),
        ("type", |v| v.work.clone()),
    ];
    for (name, key) in by {
        out.push(row(
            name,
            "calls",
            "cost",
            "median wall",
            "abstained",
            "per ruling",
        ));
        for (group, share) in shares(&voted, key) {
            out.push(row(
                &format!("  {group}"),
                &share.calls.to_string(),
                &format!("${:.2}{}", share.cost + 0.0, unpriced(share.unpriced)),
                &median(&share.seconds).map_or_else(|| "-".to_owned(), |s| format!("{s:.0}s")),
                &share.abstained.to_string(),
                &per(share.cost, share.rulings.len()),
            ));
        }
    }
    out
}

// The votes in `calls` grouped by `key`.
fn shares(calls: &[&&CallLine], key: Key) -> BTreeMap<String, Share> {
    let mut shares: BTreeMap<String, Share> = BTreeMap::new();
    for call in calls {
        let marks = call.votes.len().max(1) as f64;
        for vote in &call.votes {
            let share = shares.entry(key(vote)).or_default();
            share.calls += 1;
            share.cost += call.cost_usd.unwrap_or(0.0) / marks;
            share.unpriced += usize::from(call.unpriced);
            share.seconds.extend(call.seconds.map(|s| s as f64));
            share.abstained += usize::from(vote.choice.is_none());
            share.rulings.insert(vote.ruling);
        }
    }
    shares
}

fn row(group: &str, calls: &str, cost: &str, wall: &str, abstained: &str, per: &str) -> String {
    format!("{group:<14}{calls:>7}{cost:>12}{wall:>13}{abstained:>11}{per:>12}")
}

// A dollar figure per ruling, or a dash with no ruling.
fn per(cost: f64, rulings: usize) -> String {
    match rulings {
        0 => "-".to_owned(),
        n => format!("${:.2}", cost / n as f64 + 0.0),
    }
}

fn unpriced(n: usize) -> String {
    match n {
        0 => String::new(),
        n => format!("+{n}"),
    }
}

fn count(n: usize, thing: &str) -> String {
    match n {
        1 => format!("1 {thing}"),
        n => format!("{n} {thing}s"),
    }
}

fn median(values: &[f64]) -> Option<f64> {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    match n {
        0 => None,
        n if n % 2 == 1 => Some(values[n / 2]),
        n => Some((values[n / 2 - 1] + values[n / 2]) / 2.0),
    }
}
