//! What `shep kelpie usage --forge` prints: calls per minute per project
//! and per kind, the busiest minutes, and each used-up answer with the
//! minute before it
//!
//! A minute is a clock minute in UTC. Only calls that reached GitHub count
//! toward a minute: a held call was never made, so it is counted apart.

use std::collections::BTreeMap;

use super::{Ended, Identity, Line};
use crate::ports::Timestamp;

/// How many of the busiest minutes are listed
const BUSIEST: usize = 10;

/// How far before a used-up answer its calls are counted, in seconds
const BEFORE: u64 = 60;

/// The plain-text report on `ledger`, its lines at or after `since` when
/// given
///
/// A used-up answer's calls before it are read from the whole ledger, so
/// `since` does not cut a window short.
pub fn report(ledger: &[Line], since: Option<Timestamp>) -> Vec<String> {
    let lines: Vec<&Line> = (ledger.iter())
        .filter(|line| since.is_none_or(|since| line.at >= since))
        .collect();
    let (Some(first), Some(last)) = (
        lines.iter().map(|l| l.at).min(),
        lines.iter().map(|l| l.at).max(),
    ) else {
        return vec!["no forge calls recorded".to_owned()];
    };
    let held = lines.iter().filter(|l| l.ended == Ended::Held).count();
    let used_up = lines.iter().filter(|l| l.ended == Ended::UsedUp).count();
    let made: Vec<&Line> = (lines.iter().copied())
        .filter(|l| l.ended != Ended::Held)
        .collect();
    let mut out = vec![format!(
        "forge calls from {} to {}: {} made, {held} held, {used_up} used up",
        time(first.0),
        time(last.0),
        made.len(),
    )];

    out.push("by project (calls, minutes with a call, a minute, busiest minute):".to_owned());
    out.extend(rates(&made, |l| l.project.clone()));
    out.push("by identity (calls, minutes with a call, a minute, busiest minute):".to_owned());
    out.extend(rates(&made, |l| l.by.name()));
    out.push("by kind (calls, minutes with a call, a minute, busiest minute):".to_owned());
    out.extend(rates(&made, |l| {
        format!("{} {}", l.kind.name(), l.api.name())
    }));

    out.push("busiest minutes:".to_owned());
    let mut minutes: BTreeMap<u64, Vec<&Line>> = BTreeMap::new();
    for line in &made {
        minutes.entry(line.at.0 / 60 * 60).or_default().push(line);
    }
    let mut busiest: Vec<_> = minutes.iter().collect();
    busiest.sort_by_key(|(start, calls)| (std::cmp::Reverse(calls.len()), **start));
    for (start, calls) in busiest.into_iter().take(BUSIEST) {
        out.push(format!("  {}: {}", time(*start), tally(calls)));
    }

    out.push("used up:".to_owned());
    let mut any = false;
    for (index, line) in ledger.iter().enumerate() {
        if line.ended != Ended::UsedUp || since.is_some_and(|since| line.at < since) {
            continue;
        }
        any = true;
        // Judged by time, not by place in the file, which runners appending
        // together do not keep in order; a call in the same second counts
        // when it was appended first.
        let before: Vec<&Line> = (ledger.iter().enumerate())
            .filter(|(i, l)| {
                l.ended != Ended::Held
                    && l.at.0 + BEFORE >= line.at.0
                    && (l.at < line.at || (l.at == line.at && *i < index))
            })
            .map(|(_, l)| l)
            .collect();
        let spent = match line.by {
            Identity::Login => line.api.name(),
            by => format!("{}, {}", line.api.name(), by.name()),
        };
        out.push(format!(
            "  {} {} {} ({spent}); the {BEFORE} seconds before: {}",
            time(line.at.0),
            line.project,
            line.kind.name(),
            tally(&before)
        ));
    }
    if !any {
        out.push("  none".to_owned());
    }
    out
}

// The calls, then the projects and the kinds among them, the most first
fn tally(calls: &[&Line]) -> String {
    let by_project = counts(calls, |l| l.project.clone());
    let by_kind = counts(calls, |l| l.kind.name());
    let list = |counts: Vec<(String, usize)>| {
        let named: Vec<String> = (counts.into_iter())
            .map(|(name, n)| format!("{name} {n}"))
            .collect();
        named.join(", ")
    };
    format!(
        "{} ({}); {}",
        match calls.len() {
            1 => "1 call".to_owned(),
            n => format!("{n} calls"),
        },
        list(by_project),
        list(by_kind)
    )
}

// How many of `calls` each name has, the most first, then by name
fn counts(calls: &[&Line], name: impl Fn(&Line) -> String) -> Vec<(String, usize)> {
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for line in calls {
        *counted.entry(name(line)).or_default() += 1;
    }
    let mut counted: Vec<_> = counted.into_iter().collect();
    counted.sort_by_key(|(name, n)| (std::cmp::Reverse(*n), name.clone()));
    counted
}

// One row for each name: its calls, the minutes it made a call in, its
// average over those minutes and its busiest minute
fn rates(calls: &[&Line], name: impl Fn(&Line) -> String) -> Vec<String> {
    let mut grouped: BTreeMap<String, BTreeMap<u64, u64>> = BTreeMap::new();
    for line in calls {
        *grouped
            .entry(name(line))
            .or_default()
            .entry(line.at.0 / 60)
            .or_default() += 1;
    }
    let mut rows: Vec<(String, u64, u64, u64)> = (grouped.into_iter())
        .map(|(name, minutes)| {
            let total = minutes.values().sum();
            let busiest = minutes.values().copied().max().unwrap_or(0);
            (name, total, minutes.len() as u64, busiest)
        })
        .collect();
    rows.sort_by_key(|(name, total, ..)| (std::cmp::Reverse(*total), name.clone()));
    (rows.into_iter())
        .map(|(name, total, minutes, busiest)| {
            let tenths = (total * 10 + minutes / 2) / minutes;
            format!(
                "  {name}: {total}, {minutes}, {}.{}, {busiest}",
                tenths / 10,
                tenths % 10
            )
        })
        .collect()
}

// `seconds` since the epoch as a UTC time, as `2026-10-10T05:31:54Z`
fn time(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map_or_else(|| seconds.to_string(), |at| at.to_string())
}
