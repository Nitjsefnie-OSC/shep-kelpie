//! What a contributor says on their pull request
//!
//! The comments from the runner's thread read (see
//! [`thread_replies`](crate::runner::thread_replies)) and the review thread
//! replies are sorted here by author, never by the words. The contribution
//! author's are heard, the maintainer's kept apart as context, and anyone else's,
//! any bot's and kelpie's own are left alone. Only ids, kinds and times are
//! recorded; a prompt reads the text again, fenced as outside text
//! (`crates/runner/src/runner/untrusted.rs`). A push is heard by
//! [`Runner::follow_contributions`] as the head moving.

use std::collections::BTreeSet;

use crate::ports::{ThreadComment, Timestamp};
use crate::runner::Runner;
use crate::state::{Said, StateError};

/// Where a comment was written
#[derive(Clone, Copy)]
enum Where {
    /// On the pull request's conversation
    Conversation,
    /// In a review thread on its diff
    Review,
}

impl Runner {
    // Records who wrote `comments` and `replies` and when, once each. `read` are
    // the pull requests whose threads were read, read again from `behind`.
    // Returns whether all was sorted; without the login that tells the maintainer's
    // words from kelpie's, nothing is recorded and the next read reaches it all again.
    pub(in crate::runner) fn hear_contributions(
        &mut self,
        comments: &[ThreadComment],
        replies: &[ThreadComment],
        read: &[u64],
        behind: Timestamp,
    ) -> Result<bool, StateError> {
        if self.state.contributions.is_empty() {
            return Ok(true);
        }
        // The logins are kept once asked, and a contribution is only followed
        // once they were, so this is not expected to fail.
        let (maintainer, viewer) = (self.maintainer().ok(), self.viewer().ok());
        if maintainer.is_none() || viewer.is_none() {
            return Ok(false);
        }
        let is = |login: &Option<String>, author: &str| {
            login
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(author))
        };
        let mut own: BTreeSet<u64> = self.state.posted.iter().copied().collect();
        own.extend(self.posted.borrow().iter().copied());
        let mut all: Vec<_> = (comments.iter().map(|c| (Where::Conversation, c)))
            .chain(replies.iter().map(|c| (Where::Review, c)))
            .collect();
        all.sort_by_key(|(_, c)| (c.created, c.id));
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        for (place, comment) in all {
            let Some(contribution) =
                (next.contributions.iter_mut()).find(|c| c.pull_request == comment.thread)
            else {
                continue;
            };
            let (id, at) = (comment.id, comment.created);
            let said = match place {
                Where::Conversation => Said::Comment { id, at },
                Where::Review => Said::Reply { id, at },
            };
            // A bot is no one's word. Kelpie's own comment through `gh`
            // carries the login `gh` acts as, which may be the maintainer's:
            // a comment's id says it was kelpie's, but a review reply's does
            // not, so that login's replies in a review thread are left alone.
            let kelpies = match place {
                Where::Conversation => own.contains(&id),
                Where::Review => is(&viewer, &comment.author),
            };
            if comment.bot || kelpies || contribution.knows(&said) {
                continue;
            }
            if comment.author.eq_ignore_ascii_case(&contribution.author) {
                if contribution.hear(said) {
                    contribution.restart_clock(at, now);
                }
            } else if is(&maintainer, &comment.author) {
                contribution.overhear(said);
            }
        }
        for contribution in &mut next.contributions {
            if read.contains(&contribution.pull_request) {
                contribution.reviews_from = Some(behind);
            }
        }
        if next != self.state {
            self.save(next)?;
        }
        Ok(true)
    }
}
