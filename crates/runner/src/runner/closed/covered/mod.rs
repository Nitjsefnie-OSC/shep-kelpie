//! Work items another pull request covers
//!
//! An open pull request that a contributor opened, and that closes a work item's issue, covers
//! it: the item ends with no change and its issue's thread names the pull request, and the
//! contribution flow takes the pull request from there. A pull request by the maintainer or by
//! the account kelpie acts as is work on another branch and keeps the item, as does one on the
//! item's own branch. An item with a pull request of its own is left alone, since the maintainer
//! then has two to choose between.

use super::end;
use crate::board::{OpenPullRequest, closes};
use crate::runner::Runner;
use crate::runner::contributions::Logins;
use crate::state::ProjectState;
use crate::ticket::TicketId;
use crate::work_item::Phase;

/// The open pull request in `open` that covers `ticket`: not on `branch`, closing `ticket`'s
/// issue, and opened by someone other than `own` or from a fork
pub(in crate::runner) fn covering(
    open: &[OpenPullRequest],
    ticket: TicketId,
    branch: &str,
    own: &Logins,
) -> Option<u64> {
    (open.iter())
        .find(|pr| {
            pr.head != branch && closes(pr, ticket) && (pr.from_fork || !own.own(&pr.author))
        })
        .map(|pr| pr.number)
}

/// Marks `ticket`'s work item in `next` to end with no change, covered by another pull request,
/// and drops its rulings. Returns the ruling it was parked on, if any, or `None` where the work
/// item is not open.
pub(in crate::runner) fn end_covered(
    next: &mut ProjectState,
    ticket: TicketId,
) -> Option<Option<TicketId>> {
    end(
        next,
        ticket,
        Phase::Done {
            merged: false,
            closed: false,
        },
    )
}

impl Runner {
    /// The open pull request in `open` that covers `ticket`'s work item on `branch`
    ///
    /// A login that cannot be read is noted and leaves the item as it is.
    pub(in crate::runner) fn covered_by(
        &mut self,
        open: &[OpenPullRequest],
        ticket: TicketId,
        branch: &str,
    ) -> Option<u64> {
        if !open.iter().any(|pr| closes(pr, ticket)) {
            return None;
        }
        covering(open, ticket, branch, &self.own_logins_noted()?)
    }

    /// The logins whose pull requests cover no work item, or none when they cannot be read,
    /// which is noted
    pub(in crate::runner) fn own_logins_noted(&mut self) -> Option<Logins> {
        match self.own_logins() {
            Ok(own) => Some(own),
            Err(e) => {
                let note = format!("cannot tell whose pull request covers a work item: {e}");
                self.notes.push(note);
                None
            }
        }
    }

    /// Logs that pull request `number` covers the work item for `ticket`, so it ends, and that
    /// ruling `id` is withdrawn, if it was parked on one. Posts it once to the webhook, where one
    /// is set: nothing is left to retry it for.
    pub(in crate::runner) fn say_covered(
        &mut self,
        ticket: TicketId,
        number: u64,
        id: Option<TicketId>,
    ) {
        let withdrawn = id.map_or_else(String::new, |id| format!(" Ruling {id} is withdrawn."));
        let text = format!(
            "Pull request #{number} closes {}, so its work item ends with no change.{withdrawn} \
             Nothing to answer.",
            ticket.named().to_lowercase()
        );
        self.announce(ticket, text, "covered by another pull request");
    }
}

#[cfg(test)]
mod tests;
