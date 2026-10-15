//! `shep kelpie usage <project> --reads [--since <date>]`: what workers read
//! before their first edit
//!
//! Each file a worker's session read before it first wrote or edited one is
//! counted once, in the first of these it fits: a file the session then
//! edited, a neighbour of one (in the folder of an edited file, or the one
//! a Rust module of that name keeps its children in), documentation, and
//! other code.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::Line;
use super::report::{median, percent};
use super::requests::{Parsed, Touch};
use super::session::{Seen, json_lines, seen, window};
use crate::ports::{Role, Timestamp};

/// The files one session read before its first edit, by what they were
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Split {
    edited: u64,
    neighbours: u64,
    docs: u64,
    other_code: u64,
}

impl Split {
    fn files(self) -> u64 {
        self.edited + self.neighbours + self.docs + self.other_code
    }
}

/// The workers on one harness
#[derive(Debug, Default, Serialize)]
struct Group {
    harness: String,
    /// Sessions that edited a file
    sessions: usize,
    /// Sessions that edited none, which are left out of the rest
    no_edit: usize,
    /// Sessions whose transcript was not read
    unread: usize,
    median_files: Option<u64>,
    edited: u64,
    neighbours: u64,
    docs: u64,
    other_code: u64,
    edited_share: Option<f64>,
    neighbours_share: Option<f64>,
    docs_share: Option<f64>,
    other_code_share: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ReadsJson<'a> {
    project: &'a str,
    since: Option<u64>,
    groups: &'a [Group],
}

/// The report on `project`'s ledger `lines` from `since` on, its workers'
/// transcripts placed by `steps`, as JSON when `json`
pub fn reads(
    project: &str,
    lines: &[Line],
    steps: &str,
    since: Option<Timestamp>,
    json: bool,
) -> Vec<String> {
    let calls: Vec<_> = window(lines, since)
        .into_iter()
        .filter(|call| call.role == Role::Worker)
        .collect();
    let sessions = seen(&calls, steps);
    let mut by: BTreeMap<&str, Vec<&Seen>> = BTreeMap::new();
    for session in &sessions {
        by.entry(&session.harness).or_default().push(session);
    }
    let groups: Vec<Group> = by
        .into_iter()
        .map(|(harness, sessions)| group(harness, &sessions))
        .collect();
    if json {
        return json_lines(&ReadsJson {
            project,
            since: since.map(|s| s.0),
            groups: &groups,
        });
    }
    if groups.is_empty() {
        return vec![format!(
            "{project}: no worker sessions recorded in the ledger"
        )];
    }
    let mut out = vec![
        format!("{project}: files workers read before their first edit, per harness"),
        row(&heads()),
    ];
    out.extend(groups.iter().map(|g| row(&cells(g))));
    let no_edit: usize = groups.iter().map(|g| g.no_edit).sum();
    let unread: usize = groups.iter().map(|g| g.unread).sum();
    if no_edit > 0 {
        out.push(format!(
            "{no_edit} sessions edited no file, and are left out"
        ));
    }
    if unread > 0 {
        out.push(format!(
            "{unread} sessions are left out: no transcript was found for them, or kelpie reads \
             none of their harness's"
        ));
    }
    out
}

fn group(harness: &str, sessions: &[&Seen]) -> Group {
    let mut splits = Vec::new();
    let mut group = Group {
        harness: harness.to_owned(),
        ..Group::default()
    };
    for session in sessions {
        match session.parsed.as_ref() {
            None => group.unread += 1,
            Some(parsed) => match split(parsed) {
                Some(split) => splits.push(split),
                None => group.no_edit += 1,
            },
        }
    }
    group.sessions = splits.len();
    group.median_files =
        median(splits.iter().map(|s| s.files() as f64).collect()).map(|files| files.round() as u64);
    group.edited = splits.iter().map(|s| s.edited).sum();
    group.neighbours = splits.iter().map(|s| s.neighbours).sum();
    group.docs = splits.iter().map(|s| s.docs).sum();
    group.other_code = splits.iter().map(|s| s.other_code).sum();
    let total = group.edited + group.neighbours + group.docs + group.other_code;
    let share = |n: u64| (total > 0).then(|| n as f64 / total as f64);
    group.edited_share = share(group.edited);
    group.neighbours_share = share(group.neighbours);
    group.docs_share = share(group.docs);
    group.other_code_share = share(group.other_code);
    group
}

// What a session read before its first edit, or none when it edited nothing
fn split(parsed: &Parsed) -> Option<Split> {
    let first = parsed
        .touches
        .iter()
        .position(|t| matches!(t, Touch::Edit(_)))?;
    let edited: BTreeSet<&str> = (parsed.touches.iter())
        .filter_map(|touch| match touch {
            Touch::Edit(path) => Some(path.as_str()),
            Touch::Read(_) => None,
        })
        .collect();
    let read: BTreeSet<&str> = (parsed.touches[..first].iter())
        .filter_map(|touch| match touch {
            Touch::Read(path) => Some(path.as_str()),
            Touch::Edit(_) => None,
        })
        .collect();
    let mut split = Split::default();
    for path in read {
        if edited.contains(path) {
            split.edited += 1;
        } else if edited.iter().any(|edit| same_module(path, edit)) {
            split.neighbours += 1;
        } else if is_doc(path) {
            split.docs += 1;
        } else {
            split.other_code += 1;
        }
    }
    Some(split)
}

// The folder a file is in, and the file's name without its extension
fn place(path: &str) -> (&str, &str) {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    (folder, name.rsplit_once('.').map_or(name, |(stem, _)| stem))
}

// Whether `a` and `b` are in one folder, or one is in the folder a Rust
// module named for the other keeps its children in: `src/usage.rs` and
// `src/usage/report.rs`
fn same_module(a: &str, b: &str) -> bool {
    let (a_folder, a_stem) = place(a);
    let (b_folder, b_stem) = place(b);
    let child_of = |folder: &str, parent_folder: &str, parent_stem: &str| {
        folder
            .strip_prefix(parent_folder)
            .is_some_and(|rest| rest.trim_start_matches('/') == parent_stem)
    };
    a_folder == b_folder
        || child_of(a_folder, b_folder, b_stem)
        || child_of(b_folder, a_folder, a_stem)
}

fn is_doc(path: &str) -> bool {
    const EXTENSIONS: [&str; 5] = ["md", "mdx", "txt", "rst", "adoc"];
    let extension = path.rsplit_once('.').map(|(_, ext)| ext);
    extension.is_some_and(|ext| EXTENSIONS.contains(&ext))
        || path.split('/').any(|dir| dir == "docs")
}

fn heads() -> [String; 8] {
    [
        "harness",
        "sessions",
        "median files",
        "edited later",
        "neighbours",
        "other code",
        "docs",
        "files",
    ]
    .map(str::to_owned)
}

fn cells(group: &Group) -> [String; 8] {
    let both = |n: u64, share: Option<f64>| format!("{n} ({})", percent(share));
    [
        group.harness.clone(),
        group.sessions.to_string(),
        group
            .median_files
            .map_or_else(|| "-".to_owned(), |n| n.to_string()),
        both(group.edited, group.edited_share),
        both(group.neighbours, group.neighbours_share),
        both(group.other_code, group.other_code_share),
        both(group.docs, group.docs_share),
        (group.edited + group.neighbours + group.docs + group.other_code).to_string(),
    ]
}

fn row(cells: &[String; 8]) -> String {
    let [
        harness,
        sessions,
        median,
        edited,
        neighbours,
        other,
        docs,
        files,
    ] = cells;
    format!(
        "{harness:<13}{sessions:>9}{median:>14}{edited:>15}{neighbours:>15}{other:>15}{docs:>13}\
         {files:>8}"
    )
}
