//! The panel's votes on a ruling
//!
//! Four experts weigh in on each ruling, each blind to the others: the
//! worker, the project manager, the advisor and the researcher. Each votes
//! with one JSON object, `{"choice": "<option key>", "why": "<at most 350
//! characters>", "design": true|false}`. A vote kelpie cannot read is an
//! abstention. This module is the votes and how one is read; the runner
//! asks for them, and [`settle`] is how the votes decide a ruling.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod tally;

pub use tally::{Advice, MAJORITY, Stage, Step, Tier, Verdict, Work, settle, simulate, work_of};

/// The most a vote's `why` keeps, in characters: four lines of a GitHub comment
pub const WHY_LONGEST: usize = 350;

/// An expert on the panel
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Voter {
    /// The work item's own session, resumed once with the ruling
    Worker,
    /// The project manager
    Pm,
    /// The advisor, who reads the codebase
    Advisor,
    /// The researcher, who reads everything outside the codebase
    Researcher,
}

impl Voter {
    /// Every expert, in the order the comment lists them
    pub const ALL: [Self; 4] = [Self::Worker, Self::Pm, Self::Researcher, Self::Advisor];

    /// What the comment calls it
    pub fn name(self) -> &'static str {
        match self {
            Self::Worker => "Worker",
            Self::Pm => "PM",
            Self::Advisor => "Advisor",
            Self::Researcher => "Researcher",
        }
    }
}

/// One expert's vote
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vote {
    /// The option's key, `A` for the first
    pub choice: char,
    /// Why, in one line of at most [`WHY_LONGEST`] characters
    pub why: String,
    /// Whether the question changes an ADR, the glossary, a public interface
    /// or the project's direction
    pub design: bool,
}

/// What an expert did on a ruling
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ballot {
    /// Who
    pub voter: Voter,
    /// Their vote, or `None` when they abstained
    pub vote: Option<Vote>,
}

/// The vote in `text`, which holds options `A` to the `options`th letter
///
/// `text` is an expert's reply: the last JSON object in it with a `choice`
/// is the vote. A vote that is not an object with all three fields, or
/// whose choice is no option's key, is an error that says why, for the log.
/// A `why` past [`WHY_LONGEST`] characters is cut there.
///
/// # Errors
///
/// Why the reply holds no vote kelpie reads.
pub fn read(text: &str, options: usize) -> Result<Vote, String> {
    let fields = last_object(text).ok_or("its reply held no JSON object with a `choice`")?;
    from_fields(&fields, options)
}

/// The vote the `fields` of a JSON object make, as [`read`] has it
///
/// # Errors
///
/// Why the object is no vote kelpie reads.
pub fn from_fields(fields: &Map<String, Value>, options: usize) -> Result<Vote, String> {
    let choice = match fields.get("choice") {
        Some(Value::String(key)) => key.trim().to_ascii_uppercase(),
        other => return Err(format!("`choice` is not a string: {other:?}")),
    };
    let mut keys = choice.chars();
    let (Some(key), None) = (keys.next(), keys.next()) else {
        return Err(format!("`choice` is `{choice}`, not one option's key"));
    };
    let valid = ('A'..='Z').take(options).any(|k| k == key);
    if !valid {
        return Err(format!(
            "`choice` is `{key}`, and there are {options} options"
        ));
    }
    let why = match fields.get("why") {
        Some(Value::String(why)) => oneline(why),
        other => return Err(format!("`why` is not a string: {other:?}")),
    };
    let Some(Value::Bool(design)) = fields.get("design") else {
        return Err(format!(
            "`design` is not true or false: {:?}",
            fields.get("design")
        ));
    };
    Ok(Vote {
        choice: key,
        why,
        design: *design,
    })
}

// The last top-level object in `text` that has a `choice`. Read from the
// start, each whole JSON value is skipped past, so nothing inside one, a
// string's braces included, is taken for a vote of its own.
fn last_object(text: &str) -> Option<Map<String, Value>> {
    let mut found = None;
    let mut at = 0;
    while let Some(offset) = text[at..].find('{') {
        let start = at + offset;
        let mut values = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
        match values.next() {
            Some(Ok(value)) => {
                at = start + values.byte_offset();
                if let Value::Object(fields) = value
                    && fields.contains_key("choice")
                {
                    found = Some(fields);
                }
            }
            _ => at = start + 1,
        }
    }
    found
}

// `why` on one line with no control character, cut past [`WHY_LONGEST`]
// characters, since it is posted as it is.
fn oneline(why: &str) -> String {
    let flat: Vec<&str> = why.split_whitespace().collect();
    let text: String = flat.join(" ").chars().filter(|c| !c.is_control()).collect();
    match text.chars().count() > WHY_LONGEST {
        true => {
            let mut cut: String = text.chars().take(WHY_LONGEST - 1).collect();
            cut.push('…');
            cut
        }
        false => text,
    }
}

#[cfg(test)]
mod tests;
