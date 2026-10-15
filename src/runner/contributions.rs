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
//! contributor's can run ([`ruling`] raises one for a flagged head). A head
//! that passed, and whose CI is green, is then read by the reviewers
//! ([`pass`]). A quiet clock runs while the head's results wait on the contributor
//! ([`clock`]). A local reviewer's pass is posted ([`post`]); nothing of the
//! contributor's runs here.
//!
//! A contribution holds no slot while it waits on its contributor. A call on
//! it takes one for as long as it runs, starts only where a slot is free, and
//! goes ahead of the board's next issue ([`Runner::contribution_due`], asked by
//! `dispatch`). Its priced cost adds to the contribution's `spend`, and past
//! `budget.per_item_usd` no call starts until a ruling is answered
//! ([`budget`]).

mod answer;
mod bots;
mod budget;
mod ci;
mod clock;
mod heard;
mod merge;
mod pass;
pub(in crate::runner) mod post;
mod release;
mod ruling;
mod security;

pub(super) use bots::waiting_on;
pub use clock::Followed;
pub(super) use ruling::{
    ADOPT_ANYWAY, CLOSE_WITH_THANKS, KEEP_WAITING, flagged_lines, keep_answer, raise_budget,
    rule_merge, settle, trust,
};

use crate::ports::{Agents, Ending, Reviewer};
use crate::state::StateError;

use super::Runner;
use super::report::Begin;
use crate::board::{OpenPullRequest, READY};
use crate::ports::ForgeError;
use crate::settings::Pick;
use crate::state::Contribution;

/// A call on a contribution's head, with everything it needs
pub(super) enum Job {
    /// The security review of the head
    Security(security::Job),
    /// The pass of the reviewers over the head
    Pass(pass::Job),
}

/// What a call on a contribution's head came to
#[derive(Debug)]
pub(super) enum Reading {
    /// A security review's
    Security(security::Reading),
    /// A pass's
    Pass(Box<pass::Walked>),
}

impl Job {
    /// The contribution it is on
    pub(super) fn pull_request(&self) -> u64 {
        match self {
            Self::Security(job) => job.pull_request,
            Self::Pass(job) => job.pull_request,
        }
    }

    /// What the thread it runs on is called
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Security(_) => "security",
            Self::Pass(_) => "pass",
        }
    }

    /// The head it reads
    pub(super) fn head(&self) -> &str {
        match self {
            Self::Security(job) => &job.head,
            Self::Pass(job) => &job.head,
        }
    }

    /// What the call leaves when it panics: the head unread, for the next poll
    pub(super) fn unread(&self) -> Reading {
        match self {
            Self::Security(_) => Reading::Security(security::Reading::default()),
            Self::Pass(_) => Reading::Pass(Box::default()),
        }
    }

    /// Runs the call to its end, or until `ending` is asked
    pub(super) fn run(
        self,
        agents: &dyn Agents,
        reviewer: &dyn Reviewer,
        ending: &Ending,
    ) -> Reading {
        match self {
            Self::Security(job) => Reading::Security(job.read(agents, ending)),
            Self::Pass(job) => Reading::Pass(Box::new(job.walk(agents, reviewer, ending))),
        }
    }
}

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
    // The call on a contribution's head to start, if one is due: its security
    // review first, then the pass over a green head. See `security_due` and
    // `run_pass`.
    pub(super) fn contribution_due(&mut self) -> Result<Option<Begin>, StateError> {
        match self.security_due()? {
            Some(begin) => Ok(Some(begin)),
            None => self.run_pass(),
        }
    }

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
        self.follow_open()?;
        // A pull request that left the record takes its checkout with it.
        self.remove_checkouts();
        Ok(())
    }

    fn follow_open(&mut self) -> Result<(), StateError> {
        if self.settings.contributions.is_off() {
            if self.state.contributions.is_empty() {
                return Ok(());
            }
            let asked = self.abandon_turns(&[]);
            let mut next = self.state.clone();
            next.follow(Vec::new());
            ruling::withdraw(&mut next);
            self.save(next)?;
            self.give_up_asks(&asked)?;
            return self.release_idle_bots();
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
        self.flights.security_retry();
        let mut followed: Vec<Contribution> = Vec::new();
        for pr in open.iter().filter(|pr| self.is_contribution(pr, &own)) {
            let known = (self.state.contributions.iter()).find(|c| c.pull_request == pr.number);
            followed.push(match known {
                Some(known) if known.head == pr.commit => known.clone(),
                // A new head is unread, and the last head that passed stays the
                // one it is read since. The push is heard.
                Some(known) => known.pushed(pr.commit.clone(), now),
                None => Contribution::new(pr.number, pr.author.clone(), pr.commit.clone(), now)
                    .with_spent(self.state.spent_by(pr.number)),
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
            let asked = self.abandon_turns(&followed);
            let mut next = self.state.clone();
            next.follow(followed);
            ruling::withdraw(&mut next);
            self.save(next)?;
            self.give_up_asks(&asked)?;
        }
        self.follow_ci()?;
        self.follow_bots()?;
        self.follow_posts()?;
        // Before the answers, so a yes or a no to the ask is not answered as
        // a conversation.
        self.follow_release()?;
        self.follow_answers()?;
        self.follow_clock()?;
        self.follow_merge()
    }
}

#[cfg(test)]
mod answer_tests;
#[cfg(test)]
pub(in crate::runner) mod budget_tests;
#[cfg(test)]
mod ci_tests;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod heard_tests;
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
pub(in crate::runner) mod pass_tests;
#[cfg(test)]
mod post_tests;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod ruling_tests;
#[cfg(test)]
pub(in crate::runner) mod security_tests;
#[cfg(test)]
pub(in crate::runner) mod tests;
