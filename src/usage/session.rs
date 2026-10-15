//! The sessions a window of the ledger's calls ran in, and what their
//! transcripts say
//!
//! The ledger names a call's session. The project's `steps.jsonl` says where
//! the harness wrote it, in the `session-started` step logged as the call
//! began, so a transcript is found by its id and never by a guessed folder.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::CallLine;
use super::requests::{Parsed, parse};
use crate::ports::Role;

/// One session, as the first of its calls in a window names it
#[derive(Debug)]
pub(super) struct Seen {
    /// Its id
    pub session: String,
    /// The role of its first call
    pub role: Role,
    /// The harness of its first call, or `unknown` for an imported line
    pub harness: String,
    /// Its transcript, where the steps file names it and the file exists
    pub file: Option<PathBuf>,
    /// What the transcript holds, when kelpie reads that harness's
    pub parsed: Option<Parsed>,
}

/// The sessions of `calls`, in the order their first calls were made, with
/// each transcript that `steps`, the project's steps file, places
///
/// A call with no session, and a local round, has none to find.
pub(super) fn seen(calls: &[&CallLine], steps: &str) -> Vec<Seen> {
    let wanted: BTreeSet<String> = (calls.iter())
        .filter_map(|call| call.session.clone())
        .collect();
    let places = crate::watch::transcripts_of(steps, &wanted);
    let mut known = BTreeSet::new();
    let mut out = Vec::new();
    for call in calls {
        let Some(session) = call
            .session
            .as_deref()
            .filter(|s| known.insert(s.to_owned()))
        else {
            continue;
        };
        let placed = places.get(session);
        let text = placed.and_then(|(_, file)| std::fs::read(file).ok());
        // A bad byte costs only its own line, which then reads as nothing.
        let text = text.map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let parsed = (placed.zip(text)).and_then(|((format, _), text)| parse(*format, &text));
        out.push(Seen {
            session: session.to_owned(),
            role: call.role,
            harness: (call.harness.clone()).unwrap_or_else(|| "unknown".to_owned()),
            file: placed.map(|(_, file)| file.clone()),
            parsed,
        });
    }
    out
}

/// The calls of `lines` that ended at or after `since`, when given
pub(super) fn window(
    lines: &[super::Line],
    since: Option<crate::ports::Timestamp>,
) -> Vec<&CallLine> {
    (lines.iter())
        .filter_map(|line| match line {
            super::Line::Call(call) if since.is_none_or(|since| call.at >= since) => Some(call),
            _ => None,
        })
        .collect()
}

/// `value` as pretty-printed JSON, a line each
pub(super) fn json_lines(value: &impl serde::Serialize) -> Vec<String> {
    match serde_json::to_string_pretty(value) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(e) => vec![format!("cannot write JSON: {e}")],
    }
}
