//! The issue check: whether an issue holds every part an implementer builds from
//!
//! It reads structure, not quality: each part is there and well formed. The
//! parts are what the issue writer and the triager write, the blockers and
//! harness reach `CLAUDE.md` asks for, and the labels the board and the
//! triager set. The issue skill, `skills/kelpie/issue/SKILL.md`, gives the
//! same format to the agent that writes an issue, and a test holds the two
//! together.

use std::fmt;

use super::is_priority_label;
use crate::guard::{CONVENTIONAL_TYPES, conventional_type};

/// The line naming the files an issue changes, each path in backticks
pub const TOUCHES: &str = "Touches:";

/// The heading of the section that says what to build
pub const WHAT_TO_BUILD: &str = "What to build";

/// The heading of the section of `- [ ]` items a pull request is checked against
pub const CRITERIA: &str = "Acceptance criteria";

/// The line saying which harnesses a change to one harness's adapter reaches
pub const HARNESS_REACH: &str = "Harness reach:";

/// The line naming the issues that must merge first
pub const BLOCKED_BY: &str = "Blocked by:";

/// What a kind label starts with, as `kind:feat`
pub const KIND: &str = "kind:";

/// The harnesses whose adapters are named for them, as in `adapters/pi/`
pub const HARNESSES: [&str; 4] = ["claude", "pi", "codex", "goose"];

/// A part of the format an issue lacks or holds malformed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The title is empty
    NoTitle,
    /// No line starts `Touches:`, and no heading is `Touches`
    NoTouches,
    /// The `Touches:` line, the list under it or the `Touches` section names
    /// no path in backticks
    TouchesNoPath,
    /// `Touches:` names this path in one harness's adapter, and no
    /// `Harness reach:` line with text follows
    NoHarnessReach(String),
    /// No `What to build` heading, or nothing under it
    NoWhatToBuild,
    /// No `Acceptance criteria` heading
    NoCriteria,
    /// The `Acceptance criteria` section holds no `- [ ]` item
    NoCriteriaItems,
    /// A `Blocked by:` line names no issue
    BlockedByNoIssue,
    /// No priority label
    NoPriority,
    /// More than one priority label, these
    Priorities(Vec<String>),
    /// No `kind:` label, and no conventional type starting the title
    NoKind,
    /// This `kind:` label names no conventional type
    UnknownKind(String),
    /// More than one `kind:` label, these
    Kinds(Vec<String>),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTitle => f.write_str("the title is empty"),
            Self::NoTouches => write!(
                f,
                "no `{TOUCHES}` line or `## Touches` section naming the files it changes"
            ),
            Self::TouchesNoPath => write!(f, "`{TOUCHES}` names no path in backticks"),
            Self::NoHarnessReach(path) => write!(
                f,
                "`{TOUCHES}` names `{path}`, one harness's adapter, and no `{HARNESS_REACH}` \
                 line says which harnesses the change reaches"
            ),
            Self::NoWhatToBuild => {
                write!(f, "no `## {WHAT_TO_BUILD}` section, or nothing under it")
            }
            Self::NoCriteria => write!(f, "no `## {CRITERIA}` section"),
            Self::NoCriteriaItems => write!(f, "the `## {CRITERIA}` section has no `- [ ]` item"),
            Self::BlockedByNoIssue => write!(f, "the `{BLOCKED_BY}` line names no issue as `#<n>`"),
            Self::NoPriority => f.write_str("no priority label, `priority: P0` to `priority: P4`"),
            Self::Priorities(labels) => {
                write!(f, "priority labels {}: keep one", ticked(labels, " and "))
            }
            Self::NoKind => write!(
                f,
                "no kind: no `{KIND}<type>` label, and the title does not start with a \
                 conventional type such as `feat:` or `fix(board):`"
            ),
            Self::UnknownKind(label) => {
                write!(
                    f,
                    "`{label}` names no conventional type, as `{KIND}feat` does"
                )
            }
            Self::Kinds(labels) => write!(f, "kind labels {}: keep one", ticked(labels, " and ")),
        }
    }
}

fn ticked<S: AsRef<str>>(labels: &[S], joint: &str) -> String {
    let ticked: Vec<String> = labels.iter().map(|l| format!("`{}`", l.as_ref())).collect();
    ticked.join(joint)
}

/// Every part of the format the issue of `title`, `body` and `labels` lacks
/// or holds malformed, none when it passes
pub fn check(title: &str, body: &str, labels: &[String]) -> Vec<Failure> {
    let mut failures = Vec::new();
    if title.trim().is_empty() {
        failures.push(Failure::NoTitle);
    }
    match touched(body) {
        None => failures.push(Failure::NoTouches),
        Some(paths) => {
            if paths.is_empty() {
                failures.push(Failure::TouchesNoPath);
            }
            let reach = labelled_line(body, HARNESS_REACH).is_some_and(|r| !bare(r).is_empty());
            let adapter = paths.into_iter().find(|p| is_adapter(p));
            if let Some(path) = adapter.filter(|_| !reach) {
                failures.push(Failure::NoHarnessReach(path.to_owned()));
            }
        }
    }
    let built = section(body, WHAT_TO_BUILD);
    if !built.is_some_and(|lines| lines.iter().any(|l| !l.trim().is_empty())) {
        failures.push(Failure::NoWhatToBuild);
    }
    match section(body, CRITERIA) {
        None => failures.push(Failure::NoCriteria),
        Some(lines) if !lines.iter().any(|l| is_item(l)) => {
            failures.push(Failure::NoCriteriaItems);
        }
        Some(_) => {}
    }
    if labelled_line(body, BLOCKED_BY).is_some_and(|line| !names_an_issue(line)) {
        failures.push(Failure::BlockedByNoIssue);
    }
    let priorities: Vec<String> = (labels.iter())
        .filter(|l| is_priority_label(l))
        .cloned()
        .collect();
    match priorities.len() {
        0 => failures.push(Failure::NoPriority),
        1 => {}
        _ => failures.push(Failure::Priorities(priorities)),
    }
    let kinds: Vec<String> = (labels.iter())
        .filter(|l| kind_of(l).is_some())
        .cloned()
        .collect();
    match kinds.as_slice() {
        [] if conventional_type(title).is_none() => failures.push(Failure::NoKind),
        [] => {}
        [label] => {
            let kind = kind_of(label).unwrap_or_default();
            if !CONVENTIONAL_TYPES
                .iter()
                .any(|t| t.eq_ignore_ascii_case(kind))
            {
                failures.push(Failure::UnknownKind(label.clone()));
            }
        }
        _ => failures.push(Failure::Kinds(kinds)),
    }
    failures
}

// The paths in backticks the issue says it touches: on its `Touches:` line,
// or the list under that line when it holds no text, or the section under a
// `Touches` heading. `None` without either.
fn touched(body: &str) -> Option<Vec<&str>> {
    let Some((at, text)) = labelled(body, TOUCHES) else {
        let name = TOUCHES.trim_end_matches(':');
        return section(body, name).map(|lines| lines.into_iter().flat_map(backticked).collect());
    };
    if !bare(text).is_empty() {
        return Some(backticked(text));
    }
    let below = body
        .lines()
        .skip(at + 1)
        .skip_while(|l| l.trim().is_empty());
    let list = below.take_while(|l| is_list_item(l));
    Some(list.flat_map(backticked).collect())
}

// Whether `line` is an item of a `-` or `*` list.
fn is_list_item(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("- ") || line.starts_with("* ")
}

/// What follows `Touches:` on the first line it starts, which may sit in a
/// list or in bold
pub fn touches_line(body: &str) -> Option<&str> {
    labelled_line(body, TOUCHES)
}

// The type a `kind:` label names, in any case and with any spaces.
fn kind_of(label: &str) -> Option<&str> {
    let (head, kind) = label.split_at_checked(KIND.len())?;
    head.eq_ignore_ascii_case(KIND).then(|| kind.trim())
}

// What follows `label` on the first line it starts, past list and bold marks.
fn labelled_line<'a>(body: &'a str, label: &str) -> Option<&'a str> {
    labelled(body, label).map(|(_, text)| text)
}

// The first line `label` starts, past list and bold marks: its index, and
// what follows the label.
fn labelled<'a>(body: &'a str, label: &str) -> Option<(usize, &'a str)> {
    body.lines().enumerate().find_map(|(at, l)| {
        let l = l.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '*');
        l.strip_prefix(label).map(|text| (at, text))
    })
}

// A line's text without the bold marks and spaces around it.
fn bare(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '*')
}

// The spans in backticks. Backticks pair up, so every second part is inside a pair.
fn backticked(line: &str) -> Vec<&str> {
    let spans = line.split('`').skip(1).step_by(2);
    spans.filter(|s| !s.trim().is_empty()).collect()
}

// Whether `path` sits in one harness's adapter: a part after an `adapters`
// folder named for a harness, as `adapters/pi/` or `adapters/codex_usage.rs`.
fn is_adapter(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    parts.windows(2).any(|pair| {
        let stem = pair[1].split(['.', '_']).next().unwrap_or_default();
        pair[0] == "adapters" && HARNESSES.contains(&stem)
    })
}

// Whether `line` names an issue: `#` then a digit, or a local ticket's `k12`.
fn names_an_issue(line: &str) -> bool {
    let number = |s: &str| s.starts_with(|c: char| c.is_ascii_digit());
    line.split('#').skip(1).any(number)
        || (line.split(|c: char| !c.is_alphanumeric())).any(|w| {
            w.strip_prefix('k')
                .is_some_and(|n| !n.is_empty() && number(n))
        })
}

// Whether `line` is a checklist item, `- [ ]` or `- [x]`.
fn is_item(line: &str) -> bool {
    let line = line.trim_start();
    let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
        return false;
    };
    ["[ ]", "[x]", "[X]"].iter().any(|b| rest.starts_with(b))
}

// The heading a line is, as its level and text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let line = line.trim();
    let level = line.chars().take_while(|&c| c == '#').count();
    let text = line[level..].strip_prefix(' ')?;
    (1..=6).contains(&level).then(|| (level, text.trim()))
}

// The lines under the heading `name`, at any level and in any case, up to the
// next heading of its level or above. `None` without that heading.
fn section<'a>(body: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let mut fenced = false;
    let mut lines = body.lines().map(|line| {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            return (line, None);
        }
        (line, heading(line).filter(|_| !fenced))
    });
    let (level, _) =
        lines.find_map(|(_, h)| h.filter(|(_, text)| text.eq_ignore_ascii_case(name)))?;
    let under = lines
        .take_while(|(_, h)| !h.is_some_and(|(l, _)| l <= level))
        .map(|(line, _)| line);
    Some(under.collect())
}
