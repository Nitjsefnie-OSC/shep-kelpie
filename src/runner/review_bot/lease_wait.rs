//! A work item waiting for a bot's lease, said once in the log
//!
//! The log names the wait when it begins, then the grant or the refusal with
//! the seconds it lasted. A pass that finds the wait going on says nothing.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::super::Runner;
use crate::ports::Timestamp;
use crate::review_bot::Bot;

/// When each work item began waiting for each bot's lease, in memory only
///
/// A restart forgets a wait, so the first pass after it says the wait again.
#[derive(Debug, Default)]
pub(in crate::runner) struct LeaseWaits(RefCell<BTreeMap<(u64, &'static str), Timestamp>>);

impl Runner {
    // Says the wait began, if this pass is the first to find it
    pub(super) fn lease_waits(&self, bot: Bot) {
        let (issue, now) = (self.item().issue, self.ports.clock.now());
        let key = (issue, bot.as_str());
        let mut waits = self.lease_waits.0.borrow_mut();
        if waits.contains_key(&key) {
            return;
        }
        waits.insert(key, now);
        drop(waits);
        let facts = self.lease_facts(bot);
        self.notes.push(format!("#{issue} lease-wait{facts}"));
    }

    // Says how long the wait lasted, if one was going on
    pub(super) fn lease_wait_ends(&self, bot: Bot, how: &str) {
        let issue = self.item().issue;
        let Some(since) = self
            .lease_waits
            .0
            .borrow_mut()
            .remove(&(issue, bot.as_str()))
        else {
            return;
        };
        let waited = self.ports.clock.now().0.saturating_sub(since.0);
        let facts = self.lease_facts(bot);
        self.notes
            .push(format!("#{issue} lease-{how}{facts} waited={waited}s"));
    }

    // The wait is over without a word, because the round ended some other way
    pub(super) fn lease_wait_forgotten(&self, bot: Bot) {
        let issue = self.current().map(|item| item.issue);
        if let Some(issue) = issue {
            self.lease_waits
                .0
                .borrow_mut()
                .remove(&(issue, bot.as_str()));
        }
    }

    fn lease_facts(&self, bot: Bot) -> String {
        let kind = self.profile(bot).lease();
        let reviewer = self.round_reviewer(bot);
        format!(" kind={kind} reviewer={reviewer}")
    }
}
