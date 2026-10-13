//! What the batch retro reads: one section per finished work item, then the
//! batch's totals
//!
//! Built in code from the records kelpie keeps (the state file's history,
//! the journal, the answered rulings and the usage ledger), so the retro
//! spends its call reading and not collecting.

use std::collections::BTreeMap;
use std::fmt::Write;

use serde::Serialize;

use super::journal::{Entry, Stamped};
use crate::panel::work_of;
use crate::runner::panel::{Answerer, Record};
use crate::state::Finished;
use crate::usage::{CallKind, CallLine, FinishedLine, Line};

mod across;

/// The most wakes listed under one work item
const WAKES_LISTED: usize = 12;

/// The most findings listed under one work item
const FINDINGS_LISTED: usize = 20;

/// The round from which a finding is called late
const LATE_ROUND: u32 = 3;

/// Everything a digest is built from
pub(super) struct Records<'a> {
    pub project: &'a str,
    /// The work items finished since the last retro, oldest first
    pub batch: &'a [Finished],
    pub journal: &'a [Stamped],
    pub rulings: &'a [Record],
    pub ledger: &'a [Line],
    /// When the last retro started, or zero with none
    pub since: u64,
    /// The retro's own earlier findings still open, by theme
    pub gaps: &'a [String],
}

/// A span of seconds as `1h05m`, `5m10s` or `42s`
pub(super) fn span(seconds: u64) -> String {
    match (seconds / 3600, seconds / 60 % 60, seconds % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

// A serialized name, as the ledger writes a role or a kind
fn name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => "?".to_owned(),
    }
}

/// The digest of `records`, in markdown
pub(super) fn build(records: &Records<'_>) -> String {
    let issues: Vec<String> = records
        .batch
        .iter()
        .map(|f| format!("#{}", f.issue))
        .collect();
    let mut out = format!(
        "# Batch digest: {}, {} finished work items ({})\n",
        records.project,
        records.batch.len(),
        issues.join(", ")
    );
    for finished in records.batch {
        out.push('\n');
        item(&mut out, records, finished);
    }
    out.push('\n');
    across::section(&mut out, records);
    totals(&mut out, records);
    out
}

fn item(out: &mut String, records: &Records<'_>, finished: &Finished) {
    let issue = finished.issue;
    let work = work_of(Some(&finished.title)).kind;
    let _ = writeln!(out, "## #{issue}: {} ({work})", finished.title);
    let end = match (finished.merged, finished.closed) {
        (true, _) => "merged",
        (_, true) => "closed with no change",
        _ => "ended without a merge",
    };
    let pull_request = finished
        .pull_request
        .map_or_else(String::new, |n| format!(", pull request #{n}"));
    let _ = writeln!(
        out,
        "- Ended: {end}{pull_request}; wall time {}",
        span(finished.wall)
    );
    if let Some(line) = ledger_line(records.ledger, finished) {
        let _ = writeln!(
            out,
            "- Ledger: {} units, ${:.2}; worker {} calls ${:.2}; reviewer {} calls ${:.2}; \
             {} review rounds, {} fix turns, {} rulings",
            line.units,
            line.cost_usd,
            line.worker.calls,
            line.worker.cost_usd,
            line.reviewer.calls,
            line.reviewer.cost_usd,
            line.review_rounds,
            line.fix_turns,
            line.rulings
        );
    }
    let mine = mine(records, finished);
    wakes(out, &mine);
    rulings(out, records, finished);
    reviews(out, &mine);
    sent(out, &mine);
    fixes(out, &mine);
    for stamped in &mine {
        if let Entry::Friction { text, .. } = &stamped.entry {
            let _ = writeln!(out, "- Friction the worker reported: {}", one_line(text));
        }
    }
}

// The journal's entries about the work item, up to its finish
fn mine<'a>(records: &Records<'a>, finished: &Finished) -> Vec<&'a Stamped> {
    (records.journal.iter())
        .filter(|s| s.at <= finished.at && s.entry.names(finished.issue))
        .collect()
}

// The item's line in the ledger: the last written at its finish
fn ledger_line<'a>(ledger: &'a [Line], finished: &Finished) -> Option<&'a FinishedLine> {
    ledger.iter().rev().find_map(|line| match line {
        Line::Finished(l) if l.issue == finished.issue && l.at == finished.at => Some(l),
        _ => None,
    })
}

fn wakes(out: &mut String, mine: &[&Stamped]) {
    let woke: Vec<&Stamped> = (mine.iter().copied())
        .filter(|s| matches!(s.entry, Entry::Wake { .. }))
        .collect();
    let _ = writeln!(out, "- Project manager wakes that named it: {}", woke.len());
    for stamped in woke.iter().take(WAKES_LISTED) {
        let Entry::Wake {
            woke_for, acted, ..
        } = &stamped.entry
        else {
            continue;
        };
        let did = match acted.is_empty() {
            true => "changed nothing".to_owned(),
            false => acted.join("; "),
        };
        let _ = writeln!(
            out,
            "  - woke for {}: {did}",
            one_line(&woke_for.join("; "))
        );
    }
    if woke.len() > WAKES_LISTED {
        let _ = writeln!(out, "  - and {} more", woke.len() - WAKES_LISTED);
    }
}

fn rulings(out: &mut String, records: &Records<'_>, finished: &Finished) {
    let asked: Vec<&Record> = (records.rulings.iter())
        .filter(|r| r.issue == Some(finished.issue) && r.at <= finished.at)
        .collect();
    let _ = writeln!(out, "- Rulings: {}", asked.len());
    for ruling in asked {
        let kind = ruling.kind.map_or("?", |k| k.as_str());
        let waited = (ruling.raised).map_or_else(String::new, |raised| {
            format!(", waited {}", span(ruling.at.0.saturating_sub(raised.0)))
        });
        let ballots: Vec<String> = (ruling.ballots.iter())
            .map(|b| {
                let vote = b
                    .vote
                    .as_ref()
                    .map_or("abstained".to_owned(), |v| v.choice.to_string());
                format!("{} {vote}", b.voter.name())
            })
            .collect();
        let votes = match ballots.is_empty() {
            true => "no panel vote".to_owned(),
            false => format!("votes: {}", ballots.join(", ")),
        };
        let by = match ruling.by() {
            Answerer::Maintainer => "maintainer",
            Answerer::Pm => "project manager",
            Answerer::Panel => "panel",
        };
        let _ = writeln!(
            out,
            "  - ruling {} ({kind}){waited}; {votes}; answered by the {by}: {}",
            ruling.id,
            one_line(&ruling.answer)
        );
    }
}

fn reviews(out: &mut String, mine: &[&Stamped]) {
    let rounds: Vec<&Entry> = (mine.iter())
        .map(|s| &s.entry)
        .filter(|e| matches!(e, Entry::Round { .. }))
        .collect();
    let latest = (rounds.iter())
        .filter_map(|e| match e {
            Entry::Round { round, .. } => Some(*round),
            _ => None,
        })
        .max();
    let latest = latest.map_or(String::new(), |n| format!(", the latest numbered {n}"));
    let _ = writeln!(out, "- Review rounds reported: {}{latest}", rounds.len());
    for entry in rounds {
        let Entry::Round {
            issue,
            round,
            reviewer,
            findings,
        } = entry
        else {
            continue;
        };
        let sent = mine.iter().find_map(|s| match &s.entry {
            Entry::FindingsSent { round: r, held, .. } if r == round => Some(*held),
            _ => None,
        });
        let sent = sent.map_or(String::new(), |held| format!(", {held} sent to the worker"));
        let _ = writeln!(
            out,
            "  - #{issue} round {round} by {reviewer}: {findings} findings{sent}"
        );
    }
}

// Each finding sent to the worker, with its round; one from a late round is
// marked, since a defect found that late was missed in the rounds before
fn sent(out: &mut String, mine: &[&Stamped]) {
    let mut lines = Vec::new();
    for stamped in mine {
        let Entry::Findings {
            round, findings, ..
        } = &stamped.entry
        else {
            continue;
        };
        let reviewer = mine.iter().find_map(|s| match &s.entry {
            Entry::Round {
                round: r, reviewer, ..
            } if r == round => Some(reviewer.as_str()),
            _ => None,
        });
        let late = if *round >= LATE_ROUND { " (late)" } else { "" };
        for found in findings {
            let line = match found.line {
                0 => String::new(),
                n => format!(":{n}"),
            };
            lines.push(format!(
                "  - round {round}{late} by {}, {}, {}{line}: {}",
                reviewer.unwrap_or("?"),
                name(&found.severity),
                found.file,
                one_line(&found.what)
            ));
        }
    }
    if lines.is_empty() {
        return;
    }
    let _ = writeln!(out, "- Findings sent to the worker:");
    for line in lines.iter().take(FINDINGS_LISTED) {
        let _ = writeln!(out, "{line}");
    }
    if lines.len() > FINDINGS_LISTED {
        let _ = writeln!(out, "  - and {} more", lines.len() - FINDINGS_LISTED);
    }
}

fn fixes(out: &mut String, mine: &[&Stamped]) {
    let mut ci = Vec::new();
    let (mut rebased, mut conflicts) = (0, Vec::new());
    for stamped in mine {
        match &stamped.entry {
            Entry::CiFailed { checks, .. } => ci.push(checks.join(", ")),
            Entry::Rebased { .. } => rebased += 1,
            Entry::Conflicted { files, .. } => conflicts.push(files.join(", ")),
            _ => {}
        }
    }
    let _ = writeln!(out, "- Fix turns after a failed check: {}", ci.len());
    for checks in ci {
        let _ = writeln!(out, "  - failed: {checks}");
    }
    let _ = writeln!(
        out,
        "- Merges of main: {rebased} rebased, {} conflicted",
        conflicts.len()
    );
    for files in conflicts {
        let _ = writeln!(out, "  - conflict in {files}");
    }
}

fn totals(out: &mut String, records: &Records<'_>) {
    let _ = writeln!(out, "## Batch totals");
    let batch: Vec<u64> = records.batch.iter().map(|f| f.issue).collect();
    let until = records
        .batch
        .iter()
        .map(|f| f.at.0)
        .max()
        .unwrap_or_default();
    // The batch's own calls, and the project manager's between the last
    // retro and the batch's end; the retro's calls are not the batch's.
    let calls: Vec<&CallLine> = (records.ledger.iter())
        .filter_map(|line| match line {
            Line::Call(call) => Some(call),
            Line::Finished(_) => None,
        })
        .filter(|call| call.kind != CallKind::Retro)
        .filter(|call| match call.issue {
            Some(issue) => batch.contains(&issue),
            None => call.at.0 > records.since && call.at.0 <= until,
        })
        .collect();
    let mut spend: BTreeMap<(String, String), (u32, u64, f64)> = BTreeMap::new();
    for call in &calls {
        let sum = spend
            .entry((name(&call.role), name(&call.kind)))
            .or_default();
        sum.0 += 1;
        sum.1 += call.units;
        sum.2 += call.cost_usd.unwrap_or_default();
    }
    let _ = writeln!(out, "- Spend by role and kind:");
    for ((role, kind), (count, units, cost)) in &spend {
        let _ = writeln!(
            out,
            "  - {role} {kind}: {count} calls, {units} units, ${cost:.2}"
        );
    }
    let wakes: Vec<&Stamped> = (records.journal.iter())
        .filter(|s| s.at.0 > records.since && s.at.0 <= until)
        .filter(|s| matches!(s.entry, Entry::Wake { .. }))
        .collect();
    let idle = (wakes.iter())
        .filter(|s| matches!(&s.entry, Entry::Wake { acted, .. } if acted.is_empty()))
        .count();
    let _ = writeln!(
        out,
        "- Project manager wakes: {}, of which {idle} changed nothing",
        wakes.len()
    );
    let waits: Vec<u64> = (records.rulings.iter())
        .filter(|r| r.issue.is_some_and(|n| batch.contains(&n)))
        .filter_map(|r| r.raised.map(|raised| r.at.0.saturating_sub(raised.0)))
        .collect();
    let asked = (records.rulings.iter())
        .filter(|r| r.issue.is_some_and(|n| batch.contains(&n)))
        .count();
    let _ = writeln!(
        out,
        "- Rulings: {asked}, waiting {} in all and {} at the longest",
        span(waits.iter().sum()),
        span(waits.iter().copied().max().unwrap_or_default())
    );
}

// Whitespace folded, so a multi-line text sits on one line
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests;
