//! A work item waiting for a bot's lease, said once in the log
//!
//! The log names the wait when it begins, then the grant or the refusal with
//! the seconds it lasted. A pass that finds the wait going on says nothing.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::super::Runner;
use crate::ports::Timestamp;
use crate::review_bot::Bot;
use crate::ticket::TicketId;

/// When each work item began waiting for each bot's lease, in memory only
///
/// A restart forgets a wait, so the first pass after it says the wait again.
#[derive(Debug, Default)]
pub(in crate::runner) struct LeaseWaits(RefCell<BTreeMap<(TicketId, &'static str), Timestamp>>);

impl Runner {
    // Says the wait began, if this pass is the first to find it
    pub(super) fn lease_waits(&self, bot: Bot) {
        let (ticket, now) = (
            self.item()
                .ticket()
                .expect("a work item's issue is never zero"),
            self.ports.clock.now(),
        );
        let key = (ticket, bot.as_str());
        let mut waits = self.lease_waits.0.borrow_mut();
        if waits.contains_key(&key) {
            return;
        }
        waits.insert(key, now);
        drop(waits);
        let facts = self.lease_facts(bot);
        self.notes.push(format!("#{ticket} lease-wait{facts}"));
    }

    // Says how long the wait lasted, if one was going on
    pub(super) fn lease_wait_ends(&self, bot: Bot, how: &str) {
        let ticket = self
            .item()
            .ticket()
            .expect("a work item's issue is never zero");
        let Some(since) = self
            .lease_waits
            .0
            .borrow_mut()
            .remove(&(ticket, bot.as_str()))
        else {
            return;
        };
        let waited = self.ports.clock.now().0.saturating_sub(since.0);
        let facts = self.lease_facts(bot);
        self.notes
            .push(format!("#{ticket} lease-{how}{facts} waited={waited}s"));
    }

    // The wait is over without a word, because the round ended some other way
    pub(super) fn lease_wait_forgotten(&self, bot: Bot) {
        let ticket = self.current().and_then(|item| item.ticket());
        if let Some(ticket) = ticket {
            self.lease_waits
                .0
                .borrow_mut()
                .remove(&(ticket, bot.as_str()));
        }
    }

    fn lease_facts(&self, bot: Bot) -> String {
        let kind = self.profile(bot).lease();
        let reviewer = self.round_reviewer(bot);
        format!(" kind={kind} reviewer={reviewer}")
    }
}
