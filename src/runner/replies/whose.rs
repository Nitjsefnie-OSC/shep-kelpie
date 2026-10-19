//! Which project's ruling a reply on the topic answers

use super::super::Runner;
use crate::home::ProjectName;
use crate::state::Projects;
use crate::state::Ruling;
use crate::ticket::TicketId;

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
    let (project, digits) = match token.rsplit_once('#') {
        None => (None, token),
        Some(("", digits)) => (None, digits),
        Some((project, digits)) => {
            ProjectName::try_from(project).ok()?;
            (Some(project), digits)
        }
    };
    let ticket: TicketId = digits.parse().ok()?;
    // A ruling is numbered by an issue or a pull request, so a ticket from another
    // source names none, and the reply is unreadable like any other token.
    ticket.github()?;
    Some(Reply {
        project,
        ticket,
        words: "",
    })
}

/// A reply read: the ruling it names, and its answer
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Reply<'a> {
    /// The project it names, if it does
    pub project: Option<&'a str>,
    /// The ticket, or the number of the pull request, a ruling is about
    pub ticket: TicketId,
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
    pub id: TicketId,
}

impl Runner {
    // The ruling `reply` names. A number is the id of the ticket, or the number
    // of the pull request, a ruling here is about, since that is what a question prints.
    pub(super) fn which(&self, reply: &Reply<'_>) -> Named {
        let Reply {
            project: named,
            ticket,
            ..
        } = *reply;
        // A ticket from another source has no pull request number, and no ruling
        // is numbered 0.
        let number = ticket.github().unwrap_or(0);
        let project = self.project.as_str();
        let about = |r: &&Ruling| {
            r.issue == Some(ticket) || ticket.github().is_some() && r.pull_request == Some(number)
        };
        let mut here: Vec<&Ruling> = self.state.rulings.iter().filter(about).collect();
        // The number names the ruling that took it, not one an older kelpie
        // left beside it under its counter id.
        if here.iter().any(|r| !r.is_left_over()) {
            here.retain(|r| !r.is_left_over());
        }
        let named_by = |whose, id| Named { whose, id };
        let id = here.first().map_or(ticket, |r| r.id);
        if named.is_some_and(|named| named != project) {
            return named_by(Whose::Theirs, id);
        }
        // A project named for none waiting is told it is settled.
        if here.is_empty() {
            let whose = if named.is_some() {
                Whose::Ours
            } else {
                Whose::Theirs
            };
            return named_by(whose, id);
        }
        if let [first, second, ..] = here.as_slice() {
            let why = format!(
                "{} has rulings {} and {} here, so answer one by its id",
                ticket.shown(),
                first.id,
                second.id
            );
            return named_by(Whose::Several(why), id);
        }
        if named.is_some() {
            return named_by(Whose::Ours, id);
        }
        for (other, state) in Projects::under(&self.paths.kelpie_home).states() {
            if other == project {
                continue;
            }
            match state {
                Ok(state) if state.rulings.iter().any(|r| about(&r)) => {
                    let why = format!("#{ticket} also has a ruling on {other}");
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
}
