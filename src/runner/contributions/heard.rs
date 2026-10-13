//! What a contributor says on their pull request
//!
//! The comments the runner's thread read finds (see
//! [`thread_replies`](crate::runner::thread_replies)) and the replies it
//! reads from the pull requests' review threads are sorted here, by author
//! and never by the words. A comment or reply by the contribution's author is
//! heard, and one by the maintainer is kept apart as context. Anyone else's,
//! any bot's, and any comment kelpie posted as the maintainer's login are
//! left alone. Only ids, kinds and times are recorded; a prompt reads the
//! text again, fenced as outside text (`src/runner/untrusted.rs`).
//!
//! A push is heard by [`Runner::follow_contributions`] as the head moving.
//! Nothing is posted and no reaction is read here.

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
    // Records what `comments`, the repo's comments since the last read, and
    // `replies`, the review threads' comments of the pull requests followed,
    // say about the contributions: who wrote them, and when. A comment on
    // record is not recorded again. `read` are the pull requests whose review
    // threads were read, which are read again from `behind`.
    //
    // Returns whether everything was sorted. When a login needed to tell the
    // maintainer's words from kelpie's and anyone else's cannot be had,
    // nothing is recorded or marked read, so the caller holds the place it
    // reads to and the next read reaches all of it again.
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
                contribution.hear(said);
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
