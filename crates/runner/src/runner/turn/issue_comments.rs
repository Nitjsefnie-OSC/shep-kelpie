//! The maintainer's comments on a work item's issue
//!
//! A worker's first prompt is the issue's title and body. What the maintainer
//! wrote under it since may change what the issue asks, so the comments they
//! wrote go after the body, oldest first. Anyone else's do not: the prompt
//! fences nothing, so it carries only words the maintainer wrote. Comments
//! posted after the first turn started reach no worker.

use jiff::tz::TimeZone;

use super::{Runner, TicketId, ticket_name};
use crate::ports::{ThreadComment, Timestamp};

/// The most the comments take of the prompt
const CAP: usize = 16 * 1024;

/// What a comment's own cut leaves room for
const CUT_NOTE_ROOM: usize = 256;

impl Runner {
    /// The comments on `ticket` its maintainer wrote, oldest first
    ///
    /// A maintainer who cannot be named is no one's, so no comment is read, and
    /// a note says why.
    ///
    /// # Errors
    ///
    /// Why the comments cannot be read, which fails the turn's start so that
    /// it is tried again and not sent without them.
    pub(super) fn maintainer_comments(
        &self,
        ticket: TicketId,
    ) -> Result<Vec<ThreadComment>, String> {
        let maintainer = match self.maintainer() {
            Ok(maintainer) => maintainer,
            Err(e) => {
                self.notes.push(format!(
                    "{}'s worker is sent no comments from the issue: the maintainer cannot be \
                     named: {e}",
                    ticket_name(ticket)
                ));
                return Ok(Vec::new());
            }
        };
        let mut comments = self
            .ports
            .tickets
            .issue_comments(&self.remote, ticket)
            .map_err(|e| format!("cannot read {} comments: {e}", ticket_name(ticket)))?;
        comments.retain(|c| !c.bot && c.author.eq_ignore_ascii_case(&maintainer));
        Ok(comments)
    }
}

/// The block of `comments` a first prompt ends with, or nothing when there are none
///
/// It keeps the newest that fit in [`CAP`] and counts the older ones it left
/// out. One comment over the cap is cut.
#[must_use]
pub(super) fn block(ticket: TicketId, comments: &[ThreadComment]) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let noun = if ticket.github().is_some() {
        "issue"
    } else {
        "ticket"
    };
    let entries: Vec<String> = comments.iter().map(entry).collect();
    let mut used = 0;
    let kept = entries
        .iter()
        .rev()
        .take_while(|e| {
            used += e.len();
            used <= CAP
        })
        .count();
    let left_out = entries.len() - kept;
    let mut text = format!("\nComments on the {noun} from the maintainer, oldest first:\n");
    if left_out > 0 {
        let (count, s) = (left_out, if left_out == 1 { "" } else { "s" });
        text.push_str(&format!(
            "\n[{count} older comment{s} left out: this list holds at most {} KiB.]\n",
            CAP / 1024
        ));
    }
    for e in &entries[left_out..] {
        text.push('\n');
        text.push_str(e);
    }
    text
}

// One comment: the day it was written, then what it says.
fn entry(comment: &ThreadComment) -> String {
    let body = comment.body.trim_end();
    let room = CAP - CUT_NOTE_ROOM;
    let said = if body.len() > room {
        let mut end = room;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n[Kelpie cut this comment here: it is {} KiB, and the list holds at most {} KiB.]",
            &body[..end],
            body.len().div_ceil(1024),
            CAP / 1024
        )
    } else {
        body.to_owned()
    };
    format!("{}:\n{said}\n", day(comment.created))
}

fn day(at: Timestamp) -> String {
    let seconds = i64::try_from(at.0).unwrap_or(i64::MAX);
    let at = jiff::Timestamp::from_second(seconds).unwrap_or(jiff::Timestamp::MAX);
    at.to_zoned(TimeZone::UTC).date().to_string()
}

#[cfg(test)]
mod tests;
