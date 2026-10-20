//! A local ticket's file: front matter, the ticket's text, then its comments
//!
//! ```text
//! ---
//! title: "Add a thing"
//! labels: ["ready-for-agent", "priority: P1"]
//! state: open
//! parent: k2
//! blocked_by: [k1]
//! ---
//! The ticket's text.
//!
//! <!-- comment: rin 1790000000 -->
//! A comment, its author and the Unix second it was written.
//! ```
//!
//! The front matter is YAML, and kelpie writes it by hand: the YAML crate it
//! reads with has no writer. A comment's marker line is what ends the text and
//! each comment before it, so no text may hold a line that starts like one.

use serde::Deserialize;

use crate::ports::IssueCloseReason;
use crate::ticket::TicketId;

/// What starts the line that begins a comment
const MARKER: &str = "<!-- comment:";

/// How a ticket stands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Still to be worked
    Open,
    /// Done, or closed with no reason
    Closed,
    /// Closed as not to be done
    NotPlanned,
    /// Closed because another ticket covers it
    Duplicate,
}

impl Standing {
    /// The standing a close for `reason` leaves
    pub fn after(reason: IssueCloseReason) -> Self {
        match reason {
            IssueCloseReason::Completed => Self::Closed,
            IssueCloseReason::NotPlanned => Self::NotPlanned,
            IssueCloseReason::Duplicate => Self::Duplicate,
        }
    }

    /// Whether the ticket is still open
    pub fn is_open(self) -> bool {
        self == Self::Open
    }

    /// The word the file holds
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::NotPlanned => "not_planned",
            Self::Duplicate => "duplicate",
        }
    }
}

/// One comment on a ticket
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Who wrote it, one word
    pub author: String,
    /// When, in Unix seconds
    pub at: u64,
    /// What it says
    pub body: String,
}

/// A ticket as its file holds it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// Its title
    pub title: String,
    /// Its labels
    pub labels: Vec<String>,
    /// How it stands
    pub standing: Standing,
    /// The ticket it is a sub-issue of
    pub parent: Option<TicketId>,
    /// The tickets that must close before it is taken
    pub blocked_by: Vec<TicketId>,
    /// Its text
    pub body: String,
    /// What was said on it, oldest first
    pub comments: Vec<Comment>,
}

// The keys of the front matter, so a misspelt one is refused by name
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Front {
    title: String,
    #[serde(default)]
    labels: Vec<String>,
    state: Standing,
    #[serde(default)]
    parent: Option<TicketId>,
    #[serde(default)]
    blocked_by: Vec<TicketId>,
}

/// Why a ticket cannot be written or read
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// A line of the text starts a comment, so reading it back would split it
    Marker,
    /// The title is empty or runs over more than one line
    Title,
    /// The file does not start with a `---` line, or its front matter has no closing one
    NoFrontMatter,
    /// The front matter is not what a ticket's is, with the parser's reason
    Front(String),
    /// A comment's marker line does not name its author and second
    Comment(String),
}

impl core::fmt::Display for FileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Marker => write!(f, "no line of a ticket's text may start with `{MARKER}`"),
            Self::Title => f.write_str("a ticket's title is one line, and not empty"),
            Self::NoFrontMatter => f.write_str(
                "must start with a `---` line, then the front matter, then a `---` line of its own",
            ),
            Self::Front(why) => write!(f, "front matter: {why}"),
            Self::Comment(line) => write!(
                f,
                "a comment's marker is `{MARKER} author seconds -->`, not `{line}`"
            ),
        }
    }
}

impl core::error::Error for FileError {}

impl Stored {
    /// A new open ticket with no comments
    pub fn new(title: &str, body: &str, labels: &[&str]) -> Self {
        Self {
            title: title.to_owned(),
            labels: labels.iter().map(|&l| l.to_owned()).collect(),
            standing: Standing::Open,
            parent: None,
            blocked_by: Vec::new(),
            body: body.to_owned(),
            comments: Vec::new(),
        }
    }

    /// The file's text
    ///
    /// # Errors
    ///
    /// [`FileError`] when the title is not one line, or a line of the text or a
    /// comment starts like a comment's marker.
    pub fn render(&self) -> Result<String, FileError> {
        let title = self.title.trim();
        if title.is_empty() || title.contains(['\n', '\r']) {
            return Err(FileError::Title);
        }
        let marked = |text: &str| text.lines().any(|l| l.trim_start().starts_with(MARKER));
        if marked(&self.body) || self.comments.iter().any(|c| marked(&c.body)) {
            return Err(FileError::Marker);
        }
        // A JSON string is a YAML double-quoted scalar, which needs no other escaping.
        let quoted = |text: &str| serde_json::Value::from(text).to_string();
        let ids = |ids: &[TicketId]| {
            let each: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
            format!("[{}]", each.join(", "))
        };
        let labels: Vec<String> = self.labels.iter().map(|l| quoted(l)).collect();
        let mut out = format!(
            "---\ntitle: {}\nlabels: [{}]\nstate: {}\n",
            quoted(title),
            labels.join(", "),
            self.standing.as_str()
        );
        if let Some(parent) = self.parent {
            out.push_str(&format!("parent: \"{parent}\"\n"));
        }
        if !self.blocked_by.is_empty() {
            out.push_str(&format!("blocked_by: {}\n", ids(&self.blocked_by)));
        }
        out.push_str("---\n");
        out.push_str(self.body.trim_end());
        out.push('\n');
        for comment in &self.comments {
            let author = comment
                .author
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-");
            out.push_str(&format!("\n{MARKER} {author} {} -->\n", comment.at));
            out.push_str(comment.body.trim_matches('\n'));
            out.push('\n');
        }
        Ok(out)
    }

    /// The ticket a file's `text` holds
    ///
    /// # Errors
    ///
    /// [`FileError`] when the front matter is missing or is not a ticket's, or a
    /// comment's marker line is malformed.
    pub fn parse(text: &str) -> Result<Self, FileError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let rest = text
            .strip_prefix("---\n")
            .or_else(|| text.strip_prefix("---\r\n"))
            .ok_or(FileError::NoFrontMatter)?;
        let mut at = 0;
        let mut end = None;
        for line in rest.split_inclusive('\n') {
            if line.trim_end_matches(['\n', '\r']) == "---" {
                end = Some((at, at + line.len()));
                break;
            }
            at += line.len();
        }
        let (front_end, body_start) = end.ok_or(FileError::NoFrontMatter)?;
        let front: Front = serde_saphyr::from_str(&rest[..front_end])
            .map_err(|e| FileError::Front(e.without_snippet().to_string()))?;
        let (body, comments) = split_comments(&rest[body_start..])?;
        Ok(Self {
            title: front.title,
            labels: front.labels,
            standing: front.state,
            parent: front.parent,
            blocked_by: front.blocked_by,
            body,
            comments,
        })
    }
}

// The text before the first comment's marker, and each comment after it
fn split_comments(text: &str) -> Result<(String, Vec<Comment>), FileError> {
    let mut body = String::new();
    let mut comments: Vec<Comment> = Vec::new();
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare.trim_start().starts_with(MARKER) {
            comments.push(marker(bare)?);
        } else if let Some(last) = comments.last_mut() {
            last.body.push_str(line);
        } else {
            body.push_str(line);
        }
    }
    for comment in &mut comments {
        comment.body = comment.body.trim_matches(['\n', '\r']).to_owned();
    }
    Ok((body.trim_end().to_owned(), comments))
}

fn marker(line: &str) -> Result<Comment, FileError> {
    let bad = || FileError::Comment(line.to_owned());
    let inner = line
        .trim()
        .strip_prefix(MARKER)
        .and_then(|rest| rest.strip_suffix("-->"))
        .ok_or_else(bad)?;
    let mut words = inner.split_whitespace();
    let (Some(author), Some(at), None) = (words.next(), words.next(), words.next()) else {
        return Err(bad());
    };
    Ok(Comment {
        author: author.to_owned(),
        at: at.parse().map_err(|_| bad())?,
        body: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> TicketId {
        text.parse().unwrap()
    }

    #[test]
    fn a_ticket_reads_back_as_it_was_written() {
        let ticket = Stored {
            title: "Add a \"thing\": now".into(),
            labels: vec!["ready-for-agent".into(), "priority: P1".into()],
            standing: Standing::NotPlanned,
            parent: Some(id("k2")),
            blocked_by: vec![id("k1"), id("340")],
            body: "First line.\n\n- a\n- b".into(),
            comments: vec![
                Comment {
                    author: "rin".into(),
                    at: 1_790_000_000,
                    body: "One.\n\nTwo.".into(),
                },
                Comment {
                    author: "kelpie".into(),
                    at: 1_790_000_060,
                    body: "Three.".into(),
                },
            ],
        };
        let text = ticket.render().unwrap();
        assert_eq!(Stored::parse(&text).unwrap(), ticket);
    }

    #[test]
    fn a_file_written_by_hand_reads() {
        let text = "---\ntitle: Fix it\nlabels:\n  - ready-for-agent\nstate: open\nparent: k3\n\
                    blocked_by: [k1, 340]\n---\nDo the thing.\n";
        let ticket = Stored::parse(text).unwrap();
        assert_eq!(ticket.title, "Fix it");
        assert_eq!(ticket.labels, ["ready-for-agent"]);
        assert_eq!(ticket.parent, Some(id("k3")));
        assert_eq!(ticket.blocked_by, [id("k1"), id("340")]);
        assert_eq!(ticket.body, "Do the thing.");
        assert!(ticket.comments.is_empty());
    }

    #[test]
    fn the_keys_after_the_title_may_be_left_out() {
        let ticket = Stored::parse("---\ntitle: T\nstate: closed\n---\n").unwrap();
        assert_eq!(ticket.standing, Standing::Closed);
        assert!(ticket.labels.is_empty() && ticket.blocked_by.is_empty());
        assert_eq!(ticket.parent, None);
        assert_eq!(ticket.body, "");
    }

    #[test]
    fn a_text_with_a_comment_marker_in_it_is_refused() {
        let mut ticket = Stored::new("T", "before\n<!-- comment: x 1 -->\nafter", &[]);
        assert_eq!(ticket.render(), Err(FileError::Marker));
        ticket.body = "fine".into();
        ticket.comments.push(Comment {
            author: "a".into(),
            at: 1,
            body: "<!-- comment: b 2 -->".into(),
        });
        assert_eq!(ticket.render(), Err(FileError::Marker));
    }

    #[test]
    fn a_title_on_two_lines_or_empty_is_refused() {
        assert_eq!(Stored::new("a\nb", "", &[]).render(), Err(FileError::Title));
        assert_eq!(Stored::new("  ", "", &[]).render(), Err(FileError::Title));
    }

    #[test]
    fn a_file_that_is_not_a_ticket_says_why() {
        assert_eq!(Stored::parse("just text\n"), Err(FileError::NoFrontMatter));
        assert_eq!(
            Stored::parse("---\ntitle: T\nstate: open\n"),
            Err(FileError::NoFrontMatter)
        );
        let typo = Stored::parse("---\ntitle: T\nstate: open\nlabel: x\n---\n").unwrap_err();
        assert!(matches!(typo, FileError::Front(why) if why.contains("label")));
        let no_state = Stored::parse("---\ntitle: T\n---\n").unwrap_err();
        assert!(matches!(no_state, FileError::Front(why) if why.contains("state")));
        let bad = Stored::parse("---\ntitle: T\nstate: open\n---\nx\n<!-- comment: rin -->\ny\n");
        assert!(matches!(bad, Err(FileError::Comment(_))));
    }
}
