//! `shep kelpie usage <project> --context [--since <date>]`: how big each
//! role's requests are, and how much of what they use is the cache

use std::collections::BTreeMap;

use serde::Serialize;

use super::Line;
use super::report::{median, percent, thousands};
use super::requests::Request;
use super::session::{Seen, json_lines, seen, window};
use crate::ports::Timestamp;

/// One role's sessions on one harness
#[derive(Debug, Default, Serialize)]
struct Group {
    role: String,
    harness: String,
    /// Sessions whose transcript was read
    sessions: usize,
    /// Sessions whose transcript was not, for want of a file or a reader
    unread: usize,
    /// Sessions whose transcript was read and holds no request
    empty: usize,
    requests: usize,
    /// The median over sessions of the context of a session's first request
    first_request: Option<u64>,
    median_request: Option<u64>,
    largest_request: Option<u64>,
    /// The share of all the requests' tokens that were cache reads
    cache_read_share: Option<f64>,
    /// The share of all the requests' tokens that were cache writes
    cache_write_share: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ContextJson<'a> {
    project: &'a str,
    since: Option<u64>,
    groups: &'a [Group],
}

/// The report on `project`'s ledger `lines` from `since` on, its sessions'
/// transcripts placed by `steps`, as JSON when `json`
pub fn context(
    project: &str,
    lines: &[Line],
    steps: &str,
    since: Option<Timestamp>,
    json: bool,
) -> Vec<String> {
    let calls = window(lines, since);
    let sessions = seen(&calls, steps);
    let mut by: BTreeMap<(String, String), Vec<&Seen>> = BTreeMap::new();
    for session in &sessions {
        let key = (session.role.as_str().to_owned(), session.harness.clone());
        by.entry(key).or_default().push(session);
    }
    let groups: Vec<Group> = by
        .into_iter()
        .map(|((role, harness), sessions)| group(role, harness, &sessions))
        .collect();
    if json {
        return json_lines(&ContextJson {
            project,
            since: since.map(|s| s.0),
            groups: &groups,
        });
    }
    if groups.is_empty() {
        return vec![format!("{project}: no sessions recorded in the ledger")];
    }
    let mut out = vec![
        format!(
            "{project}: request context (input + cache reads + cache writes), tokens, per role and harness"
        ),
        row(&heads()),
    ];
    out.extend(groups.iter().map(|g| row(&cells(g))));
    let unread: usize = groups.iter().map(|g| g.unread).sum();
    let empty: usize = groups.iter().map(|g| g.empty).sum();
    let all = unread + empty + groups.iter().map(|g| g.sessions).sum::<usize>();
    if unread > 0 {
        out.push(format!(
            "{unread} of {all} sessions are left out: no transcript was found for them, or kelpie \
             reads none of their harness's"
        ));
    }
    if empty > 0 {
        out.push(format!(
            "{empty} of {all} sessions are left out: their transcript was read and holds no request"
        ));
    }
    out
}

fn group(role: String, harness: String, sessions: &[&Seen]) -> Group {
    let parsed: Vec<&Vec<Request>> = (sessions.iter())
        .filter_map(|s| s.parsed.as_ref().map(|p| &p.requests))
        .collect();
    let read: Vec<&Vec<Request>> = (parsed.iter().copied())
        .filter(|requests| !requests.is_empty())
        .collect();
    let requests: Vec<&Request> = read.iter().flat_map(|r| r.iter()).collect();
    let whole = |value: Option<f64>| value.map(|v| v.round() as u64);
    let total: u64 = requests.iter().map(|r| r.tokens()).sum();
    let share = |tokens: u64| (total > 0).then(|| tokens as f64 / total as f64);
    Group {
        role,
        harness,
        sessions: read.len(),
        unread: sessions.len() - parsed.len(),
        empty: parsed.len() - read.len(),
        requests: requests.len(),
        first_request: whole(median(
            read.iter()
                .filter_map(|r| r.first())
                .map(|r| r.context() as f64)
                .collect(),
        )),
        median_request: whole(median(
            requests.iter().map(|r| r.context() as f64).collect(),
        )),
        largest_request: requests.iter().map(|r| r.context()).max(),
        cache_read_share: share(requests.iter().map(|r| r.usage.cache_read).sum()),
        cache_write_share: share(requests.iter().map(|r| r.usage.cache_write).sum()),
    }
}

fn heads() -> [String; 9] {
    [
        "role",
        "harness",
        "sessions",
        "requests",
        "first",
        "median",
        "largest",
        "cache read",
        "cache write",
    ]
    .map(str::to_owned)
}

fn cells(group: &Group) -> [String; 9] {
    let tokens = |n: Option<u64>| n.map_or_else(|| "-".to_owned(), thousands);
    [
        group.role.clone(),
        group.harness.clone(),
        group.sessions.to_string(),
        group.requests.to_string(),
        tokens(group.first_request),
        tokens(group.median_request),
        tokens(group.largest_request),
        percent(group.cache_read_share),
        percent(group.cache_write_share),
    ]
}

fn row(cells: &[String; 9]) -> String {
    let [
        role,
        harness,
        sessions,
        requests,
        first,
        median,
        largest,
        read,
        write,
    ] = cells;
    format!(
        "{role:<16}{harness:<13}{sessions:>9}{requests:>10}{first:>10}{median:>10}{largest:>10}\
         {read:>12}{write:>13}"
    )
}
