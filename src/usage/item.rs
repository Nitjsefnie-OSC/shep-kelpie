//! `shep kelpie usage <project> --item <issue>`: one work item's calls in
//! order, and the transcripts of their sessions

use serde::Serialize;

use super::report::{count, money, thousands};
use super::session::{Seen, json_lines, seen};
use super::{CallLine, Line};

/// One session of the work item, and where its transcript is
#[derive(Debug, Serialize)]
struct SessionJson<'a> {
    session: &'a str,
    role: &'a str,
    harness: &'a str,
    transcript: Option<String>,
    /// How many requests the transcript records, where kelpie reads it
    requests: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ItemJson<'a> {
    project: &'a str,
    issue: u64,
    pull_request: Option<u64>,
    calls: &'a [&'a CallLine],
    sessions: Vec<SessionJson<'a>>,
}

/// The report on the work item of `issue` in `project`'s ledger `lines`,
/// its sessions' transcripts placed by `steps`, as JSON when `json`
pub fn item(project: &str, lines: &[Line], steps: &str, issue: u64, json: bool) -> Vec<String> {
    let calls: Vec<&CallLine> = (lines.iter())
        .filter_map(|line| match line {
            Line::Call(call) if call.issue == Some(issue) => Some(call),
            _ => None,
        })
        .collect();
    let pull_request = calls.iter().rev().find_map(|call| call.pull_request);
    if calls.is_empty() {
        return match json {
            true => json_lines(&ItemJson {
                project,
                issue,
                pull_request,
                calls: &[],
                sessions: Vec::new(),
            }),
            false => vec![format!("{project}: no calls recorded for #{issue}")],
        };
    }
    let sessions = seen(&calls, steps);
    if json {
        let sessions = sessions.iter().map(session_json).collect();
        return json_lines(&ItemJson {
            project,
            issue,
            pull_request,
            calls: &calls,
            sessions,
        });
    }
    let pr = pull_request.map_or_else(String::new, |n| format!(" (pull request #{n})"));
    let units = calls.iter().map(|call| call.units).sum();
    let dollars = calls.iter().filter_map(|call| call.cost_usd).sum();
    let unpriced = calls.iter().filter(|call| call.unpriced).count();
    let mut out = vec![format!(
        "{project} #{issue}{pr}: {}, {} units, {}{}",
        count(calls.len(), "call"),
        thousands(units),
        money(dollars),
        match unpriced {
            0 => String::new(),
            n => format!(" + {}", count(n, "unpriced call")),
        }
    )];
    out.push(row(&Cells::heads()));
    out.extend(calls.iter().map(|call| row(&Cells::of(call))));
    out.push(String::new());
    out.push("sessions and their transcripts:".to_owned());
    for session in &sessions {
        out.push(session_line(session));
    }
    out
}

fn session_json(session: &Seen) -> SessionJson<'_> {
    SessionJson {
        session: &session.session,
        role: session.role.as_str(),
        harness: &session.harness,
        transcript: session.file.as_ref().map(|f| f.display().to_string()),
        requests: session.parsed.as_ref().map(|p| p.requests.len()),
    }
}

fn session_line(session: &Seen) -> String {
    let what = format!(
        "  {} {} ({})",
        session.session,
        session.role.as_str(),
        session.harness
    );
    match (&session.file, &session.parsed) {
        (None, _) => format!("{what}: no transcript found"),
        (Some(file), None) => format!(
            "{what}: {} (kelpie reads no requests of it)",
            file.display()
        ),
        (Some(file), Some(parsed)) => format!(
            "{what}: {}, {}",
            file.display(),
            count(parsed.requests.len(), "request")
        ),
    }
}

// One call's row, the cells already written
struct Cells([String; 13]);

impl Cells {
    fn heads() -> Self {
        let heads = [
            "at",
            "role",
            "kind",
            "harness",
            "model",
            "input",
            "cache write",
            "cache read",
            "output",
            "units",
            "dollars",
            "seconds",
            "ended",
        ];
        Self(heads.map(str::to_owned))
    }

    fn of(call: &CallLine) -> Self {
        let kind = serde_json::to_value(call.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let ended = serde_json::to_value(call.ended)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let dash = |text: &Option<String>| text.clone().unwrap_or_else(|| "-".to_owned());
        Self([
            jiff::Timestamp::from_second(i64::try_from(call.at.0).unwrap_or(0))
                .map_or_else(|_| "-".to_owned(), |at| at.to_string()),
            call.role.as_str().to_owned(),
            kind,
            dash(&call.harness),
            dash(&call.model),
            thousands(call.usage.input),
            thousands(call.usage.cache_write),
            thousands(call.usage.cache_read),
            thousands(call.usage.output),
            thousands(call.units),
            call.cost_usd.map_or_else(|| "-".to_owned(), money),
            call.seconds
                .map_or_else(|| "-".to_owned(), |s| s.to_string()),
            ended,
        ])
    }
}

fn row(cells: &Cells) -> String {
    let [
        at,
        role,
        kind,
        harness,
        model,
        input,
        write,
        read,
        output,
        units,
        dollars,
        seconds,
        ended,
    ] = &cells.0;
    format!(
        "{at:<21}{role:<10}{kind:<12}{harness:<13}{model:<20}{input:>9}{write:>13}{read:>12}\
         {output:>9}{units:>10}{dollars:>9}{seconds:>9}  {ended}"
    )
}
