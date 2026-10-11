//! What the stream shows: a time, who, what kind of line it is, and its text

use std::fmt;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde_json::Value;

/// How many characters a clipped line keeps of its text
const CLIP: usize = 200;

/// The width of the column that names who a line is from
const WHO: usize = 24;

/// What a line says
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// What kelpie sent the agent
    Prompt,
    /// What the agent said
    Said,
    /// What the agent thought
    Thinking,
    /// A tool the agent called
    Tool,
    /// What a tool answered
    Result,
    /// What the runner did
    Step,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Said => "said",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Result => "result",
            Self::Step => "step",
        }
    }
}

/// One thing a transcript or the runner's log recorded, before it is
/// attributed
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Event {
    pub at: Timestamp,
    pub kind: Kind,
    pub text: String,
}

/// One line of the stream
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub(super) at: Timestamp,
    pub(super) who: String,
    pub(super) kind: Kind,
    pub(super) text: String,
    pub(super) full: bool,
    pub(super) zone: TimeZone,
}

impl Line {
    /// When it was written
    pub fn at(&self) -> Timestamp {
        self.at
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = self.at.to_zoned(self.zone.clone()).strftime("%H:%M:%S");
        let head = format!("{time} {:<WHO$} {:<8} ", self.who, self.kind.as_str());
        if !self.full {
            return write!(f, "{head}{}", clip(&self.text));
        }
        let indent = " ".repeat(head.chars().count());
        for (n, text) in self.text.lines().enumerate() {
            match n {
                0 => write!(f, "{head}{text}")?,
                _ => write!(f, "\n{indent}{}", text.trim_end())?,
            }
        }
        if self.text.is_empty() {
            write!(f, "{head}")?;
        }
        Ok(())
    }
}

/// `text` on one line, cut short with an ellipsis
fn clip(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(CLIP) {
        Some((at, _)) => format!("{}…", &flat[..at]),
        None => flat,
    }
}

/// The text of a message's content: a string, or the text blocks of a list
pub(super) fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| match block {
                Value::String(text) => Some(text.as_str()),
                _ => block["text"].as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        // One block
        Value::Object(_) => content["text"].as_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// A tool call as one line: its name and the part of its input that says
/// what it does
pub(super) fn tool_line(name: &str, input: &Value) -> String {
    const KEYS: [&str; 9] = [
        "command",
        "cmd",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
    ];
    let said = KEYS.iter().find_map(|key| match &input[key] {
        Value::String(text) => Some(text.clone()),
        Value::Array(words) => Some(
            words
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    });
    match (said, input) {
        (Some(said), _) => format!("{name}  {said}"),
        (None, Value::Null) => name.to_owned(),
        (None, input) => format!("{name}  {input}"),
    }
}
