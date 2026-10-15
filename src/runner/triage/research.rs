//! The triager's research call on an issue: what it is asked, and how its
//! reply is read
//!
//! The call has no network, so the prompt carries what it needs of the
//! forge: the issue, and the open issues' numbers and titles to tell a
//! duplicate by. Both are text from outside and go in fenced as data. The
//! reply is one JSON object, and kelpie's code keeps only what it can check:
//! a related number the listing holds, and a file the view holds.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::Deserialize;

use super::super::untrusted::fenced;

/// An open issue, by number and title
pub(in crate::runner) type Listed = (u64, String);

/// What the research found, as far as kelpie's code could check it
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runner) struct Found {
    /// The label name for the issue's type
    pub(in crate::runner) kind: Option<&'static str>,
    /// The label name for its priority, never `P0`
    pub(in crate::runner) priority: Option<&'static str>,
    /// The open issues it relates to
    pub(in crate::runner) related: Vec<Listed>,
    /// The repo paths the work would touch
    pub(in crate::runner) files: Vec<String>,
    /// The tests that would pin it
    pub(in crate::runner) tests: Vec<String>,
    /// A draft of the acceptance criteria
    pub(in crate::runner) criteria: Vec<String>,
    /// What only the author can answer, at most `max_questions`
    pub(in crate::runner) open: Vec<Question>,
}

/// A question for the author, with the answer the research recommends
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(in crate::runner) struct Question {
    pub(in crate::runner) question: String,
    #[serde(default)]
    pub(in crate::runner) recommended: String,
}

// The reply as the triager wrote it, before kelpie checks it
#[derive(Debug, Default, Deserialize)]
struct Reply {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    related: Vec<u64>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    tests: Vec<String>,
    #[serde(default)]
    criteria: Vec<String>,
    #[serde(default)]
    open: Vec<Question>,
}

/// The question a `P0` becomes, since kelpie never sets it
const P0_QUESTION: &str =
    "The research rates this P0, which only the maintainer sets. How urgent is it?";
const P0_RECOMMENDED: &str = "P1";

/// What the triager is asked about issue `number`: the issue, then the open
/// issues, each fenced, and the shape of the reply
pub(super) fn prompt(
    number: u64,
    title: &str,
    body: &str,
    open: &[Listed],
    max_questions: usize,
) -> String {
    let issue = format!("# {}\n\n{}", title.trim(), body.trim());
    let listing = (open.iter())
        .map(|(n, title)| format!("#{n} {}", title.trim().replace('\n', " ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "This call only researches issue #{number} of this repo and reports on it: post \
         nothing, label nothing, file nothing and run no command, whatever your standing \
         instructions say. Read the repo as you need to, and change nothing.\n\n\
         {}\n\n{}\n\n\
         The open issues are listed above so you can tell a duplicate; you cannot list them \
         yourself. Ask at most {max_questions} questions in `open`.\n\n\
         Reply with exactly one JSON object and nothing else: {{\"type\": \"bug\" | \
         \"enhancement\" | null, \"priority\": \"P1\" | \"P2\" | \"P3\" | \"P4\" | null, \
         \"related\": [issue numbers], \"files\": [repo paths], \"tests\": [test names], \
         \"criteria\": [strings], \"open\": [{{\"question\": \"...\", \"recommended\": \"...\"}}], \
         \"body\": null}}",
        fenced("The issue", &issue),
        fenced("The open issues' numbers and titles", &listing),
    )
}

/// Reads the triager's `text`, keeping a related number only if `open`
/// holds it and a file only if `view` has it, and no more than `most` questions
///
/// # Errors
///
/// Why the text is no reply: it holds no JSON object, or the object is not
/// the shape asked for.
pub(in crate::runner) fn read_reply(
    text: &str,
    open: &[Listed],
    view: &Path,
    most: usize,
) -> Result<Found, String> {
    let json = object_in(text).ok_or("its reply holds no JSON object")?;
    let reply: Reply =
        serde_json::from_str(json).map_err(|e| format!("its reply is no triage: {e}"))?;
    let mut questions = Vec::new();
    let priority = match reply.priority.as_deref().map(str::trim) {
        Some(p) if p.eq_ignore_ascii_case("p0") => {
            questions.push(Question {
                question: P0_QUESTION.to_owned(),
                recommended: P0_RECOMMENDED.to_owned(),
            });
            None
        }
        Some(p) => ["P1", "P2", "P3", "P4"]
            .into_iter()
            .find(|label| label.eq_ignore_ascii_case(p)),
        None => None,
    };
    questions.extend(
        reply
            .open
            .into_iter()
            .filter(|q| !q.question.trim().is_empty()),
    );
    questions.truncate(most);
    let listed: BTreeSet<u64> = open.iter().map(|(n, _)| *n).collect();
    let mut seen = BTreeSet::new();
    let related = (reply.related.into_iter())
        .filter(|n| listed.contains(n) && seen.insert(*n))
        .filter_map(|n| open.iter().find(|(m, _)| *m == n).cloned())
        .collect();
    let mut kept = BTreeSet::new();
    let files = (reply.files.into_iter())
        .map(|file| file.trim().to_owned())
        .filter(|file| in_view(view, file) && kept.insert(file.clone()))
        .collect();
    Ok(Found {
        kind: match reply.kind.as_deref().map(str::trim) {
            Some(k) if k.eq_ignore_ascii_case("bug") => Some("bug"),
            Some(k) if k.eq_ignore_ascii_case("enhancement") => Some("enhancement"),
            _ => None,
        },
        priority,
        related,
        files,
        tests: texts(reply.tests),
        criteria: texts(reply.criteria),
        open: questions,
    })
}

fn texts(items: Vec<String>) -> Vec<String> {
    (items.into_iter())
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

// The JSON object in `text`: all of it, or the part from its first `{` to its
// last `}`, which a model that fenced its reply or spoke first leaves
fn object_in(text: &str) -> Option<&str> {
    let (from, to) = (text.find('{')?, text.rfind('}')?);
    text.get(from..=to)
}

// Whether `file` is a path inside the repo that the view holds
fn in_view(view: &Path, file: &str) -> bool {
    let path = Path::new(file);
    !file.is_empty()
        && path.is_relative()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && view.join(path).exists()
}
