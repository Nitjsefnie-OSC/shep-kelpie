//! Following the pull requests other people open
//!
//! A pull request from someone else is a contribution when it is open, not a
//! draft, and its author is not the maintainer, not the account kelpie acts
//! as, and not a bot. Under `contributions.pick = "label"` it also carries
//! `ready-for-agent`, under `"all"` it needs no label, and under `"off"`
//! nothing is one. The author and the pull request's own state decide, never
//! its branch's name: a fork can name its branch `kelpie/5`.
//!
//! Once a [`BOARD_POLL`](super::BOARD_POLL), whether or not a slot is free,
//! the runner records each contribution it sees, with its head, and drops
//! the record of one that was closed, merged, made a draft again, or, under
//! `"label"`, lost the label. A new head updates the record, and each head no
//! security review has read is read ([`security`]) before anything of the
//! contributor's can run. Nothing is posted and nothing runs for a
//! contribution here.

mod security;

use super::Runner;
use crate::board::{OpenPullRequest, READY};
use crate::ports::ForgeError;
use crate::settings::Pick;
use crate::state::{Contribution, StateError};

/// The two logins whose pull requests are not contributions
pub(super) struct Logins {
    maintainer: String,
    viewer: String,
}

impl Logins {
    fn own(&self, author: &str) -> bool {
        author.eq_ignore_ascii_case(&self.maintainer) || author.eq_ignore_ascii_case(&self.viewer)
    }
}

impl Runner {
    // The logins whose pull requests are not contributions: the maintainer's,
    // and the account kelpie acts as
    pub(super) fn own_logins(&mut self) -> Result<Logins, ForgeError> {
        Ok(Logins {
            maintainer: self.maintainer()?,
            viewer: self.viewer()?,
        })
    }

    // Whether `pr` is a contribution under the project's `contributions.pick`
    pub(super) fn is_contribution(&self, pr: &OpenPullRequest, own: &Logins) -> bool {
        let labelled = pr.labels.iter().any(|l| l == READY);
        let picked = match self.settings.contributions.pick {
            Pick::Off => return false,
            Pick::Label => labelled,
            Pick::All => true,
        };
        picked && !pr.draft && self.by_someone_else(pr, own)
    }

    // Whether another person opened `pr`, ready or not: the contributions
    // pick, and adoption leaves alone, a draft of theirs too, so that it is
    // not refused before it is marked ready.
    pub(super) fn by_someone_else(&self, pr: &OpenPullRequest, own: &Logins) -> bool {
        !self.settings.contributions.is_off() && !pr.author_is_bot && !own.own(&pr.author)
    }

    // Brings the contributions the state records in line with the open pull
    // requests, once a poll. A listing or a login that cannot be read leaves
    // the records as they are, for the board's own read to report.
    pub(super) fn follow_contributions(&mut self) -> Result<(), StateError> {
        if self.settings.contributions.is_off() {
            if self.state.contributions.is_empty() {
                return Ok(());
            }
            let mut next = self.state.clone();
            next.contributions.clear();
            return self.save(next);
        }
        let now = self.ports.clock.now();
        if !self.looks.contributions_due(now) {
            return Ok(());
        }
        let Ok(open) = self.open_listed() else {
            return Ok(());
        };
        let Ok(own) = self.own_logins() else {
            return Ok(());
        };
        let mut followed: Vec<Contribution> = Vec::new();
        for pr in open.iter().filter(|pr| self.is_contribution(pr, &own)) {
            let known = (self.state.contributions.iter()).find(|c| c.pull_request == pr.number);
            followed.push(match known {
                Some(known) if known.head == pr.commit => known.clone(),
                // A new head is unread, and the last head that passed stays the
                // one it is read since.
                _ => Contribution {
                    passed: known.and_then(|k| k.passed.clone()),
                    ..Contribution::new(pr.number, pr.author.clone(), pr.commit.clone(), now)
                },
            });
        }
        // Oldest first: the ones already followed keep their places
        let place = |c: &Contribution| {
            let held =
                (self.state.contributions.iter()).position(|k| k.pull_request == c.pull_request);
            (held.unwrap_or(usize::MAX), c.pull_request)
        };
        followed.sort_by_key(place);
        if followed != self.state.contributions {
            let mut next = self.state.clone();
            next.contributions = followed;
            self.save(next)?;
        }
        self.read_securities()
    }
}

#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod tests;
