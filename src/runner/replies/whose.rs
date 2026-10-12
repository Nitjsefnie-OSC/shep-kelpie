//! Which project's ruling a reply on the topic answers

use super::super::trigger::number;
use super::super::{ProjectName, Runner};
use crate::state::ids::RulingIds;
use crate::state::{ProjectState, Ruling};

/// A reply's text without its code, read as the ruling it names and the
/// answer's words
pub(super) fn read_reply(text: &str) -> Option<Reply<'_>> {
    let text = without_command(text.trim());
    let (first, rest) = text.split_once(char::is_whitespace)?;
    let (named, words) = match numbered(first) {
        Some(named) => (named, rest),
        None => {
            let (second, words) = rest.trim_start().split_once(char::is_whitespace)?;
            let named = numbered(second).filter(|named| named.project.is_none())?;
            let project = Some(first);
            (Reply { project, ..named }, words)
        }
    };
    let words = words.trim();
    (!words.is_empty()).then_some(Reply { words, ..named })
}

// `14`, `#14` or `koji#14`, with no words yet.
fn numbered(token: &str) -> Option<Reply<'_>> {
    let (project, digits, by_number) = match token.rsplit_once('#') {
        None => (None, token, false),
        Some(("", digits)) => (None, digits, true),
        Some((project, digits)) => {
            ProjectName::try_from(project).ok()?;
            (Some(project), digits, true)
        }
    };
    Some(Reply {
        project,
        number: number(digits)?,
        by_number,
        words: "",
    })
}

/// A reply read: the ruling it names, and its answer
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Reply<'a> {
    /// The project it names, if it does
    pub project: Option<&'a str>,
    /// A ruling's id, or the number of its issue or pull request
    pub number: u64,
    /// Whether a `#` says `number` is an issue's or a pull request's
    pub by_number: bool,
    /// The answer's words
    pub words: &'a str,
}

// A reply written as the terminal command, without the words that call it.
// Only the exact words go: `shep-kelpie 40 yes` names a project.
fn without_command(text: &str) -> &str {
    let mut words = text.split_whitespace();
    let called = match (words.next(), words.next(), words.next()) {
        (Some("shep"), Some("kelpie"), Some("rule")) => 3,
        (Some("shep-kelpie"), Some("rule"), _) => 2,
        _ => return text,
    };
    let rest = (0..called).fold(text, |rest, _| {
        let rest = rest.trim_start();
        rest.split_once(char::is_whitespace)
            .map_or("", |(_, after)| after)
    });
    rest.trim_start()
}

/// Whose ruling a reply names
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Whose {
    /// This project's
    Ours,
    /// Another project's, or none open here: the reply is left alone
    Theirs,
    /// Open here, and maybe another project's too, for this reason
    Unsure(String),
    /// Several open here on one number, for this reason
    Several(String),
}

/// The ruling a reply names: whose it is, and the id it has here
pub(super) struct Named {
    pub whose: Whose,
    pub id: u64,
    /// Whether it was found by the number of its issue or pull request
    pub by_number: bool,
}

impl Runner {
    // The ruling `reply` names. A number is a ruling's id when a ruling is
    // open under it, here or (unnamed) on another project, or when none here is about
    // it. Otherwise it is the number of the issue or pull request that the
    // ruling here is about.
    pub(super) fn which(&self, reply: &Reply<'_>) -> Named {
        let Reply {
            project: named,
            number,
            by_number,
            ..
        } = *reply;
        let project = self.project.as_str();
        let about = |r: &&Ruling| r.issue == Some(number) || r.pull_request == Some(number);
        let here: Vec<&Ruling> = self.state.rulings.iter().filter(about).collect();
        let ids = RulingIds::under(&self.paths.kelpie_home);
        // A reply that names this project is ignored by the others, so a
        // ruling of theirs under the number does not make it an id.
        let counter = !by_number
            && (here.is_empty()
                || self.state.rulings.iter().any(|r| r.id == number)
                || (named.is_none() && open_elsewhere(&ids, project, number)));
        if counter {
            return Named {
                whose: self.whose(named, number),
                id: number,
                by_number: false,
            };
        }
        let named_by = |whose, id| Named {
            whose,
            id,
            by_number: true,
        };
        let id = here.first().map_or(number, |r| r.id);
        if named.is_some_and(|named| named != project) || here.is_empty() {
            return named_by(Whose::Theirs, id);
        }
        if let [first, second, ..] = here.as_slice() {
            let why = format!(
                "#{number} has rulings {} and {} here, so answer one by its id",
                first.id, second.id
            );
            return named_by(Whose::Several(why), id);
        }
        if named.is_some() {
            return named_by(Whose::Ours, id);
        }
        for (other, state) in ids.states() {
            if other == project {
                continue;
            }
            match state {
                Ok(state) if state.rulings.iter().any(|r| about(&r)) => {
                    let why = format!("#{number} also has a ruling on {other}");
                    return named_by(Whose::Unsure(why), id);
                }
                Ok(_) => {}
                Err(_) => {
                    let why = format!("{other}'s rulings cannot be read");
                    return named_by(Whose::Unsure(why), id);
                }
            }
        }
        named_by(Whose::Ours, id)
    }

    // Whose ruling `id` is, for a reply naming `named` or no project. An id
    // from before claims is this project's when named and within its ids.
    // Unnamed, it is when open here, unsure when another project may hold it.
    pub(super) fn whose(&self, named: Option<&str>, id: u64) -> Whose {
        let project = self.project.as_str();
        if named.is_some_and(|named| named != project) {
            return Whose::Theirs;
        }
        let ids = RulingIds::under(&self.paths.kelpie_home);
        if let Some(owner) = ids.owner(id) {
            return if owner == project {
                Whose::Ours
            } else {
                Whose::Theirs
            };
        }
        let open_here = self.state.rulings.iter().any(|r| r.id == id);
        match named {
            Some(_) if id <= self.state.last_ruling => Whose::Ours,
            Some(_) => Whose::Theirs,
            None if !open_here => Whose::Theirs,
            None => {
                for (other, state) in ids.states() {
                    if other == project {
                        continue;
                    }
                    match state {
                        Ok(state) if state.rulings.iter().any(|r| r.id == id) => {
                            return Whose::Unsure(format!(
                                "ruling {id} is also waiting on {other}"
                            ));
                        }
                        Ok(_) => {}
                        Err(_) => {
                            return Whose::Unsure(format!("{other}'s rulings cannot be read"));
                        }
                    }
                }
                Whose::Ours
            }
        }
    }
}

// Whether another project has a ruling open with the id `id`.
fn open_elsewhere(ids: &RulingIds, project: &str, id: u64) -> bool {
    let open = |state: ProjectState| state.rulings.iter().any(|r| r.id == id);
    ids.states()
        .into_iter()
        .any(|(other, state)| other != project && state.is_ok_and(open))
}
