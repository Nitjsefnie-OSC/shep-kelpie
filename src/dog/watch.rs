//! What the dog's leases come to, for the metrics dog
//!
//! The desk shows each lease's holders and queue after every change, and this
//! works out what moved: who was granted since last time, and how long they
//! waited. A grant is seen as a holder that was not there before, so every way
//! the book and the counted leases grant is counted by one rule.

use std::collections::{BTreeMap, BTreeSet};

use super::desk::Desk;
use crate::lease::counted::{CARGO_TEST, CountedStatus, Ticket};
use crate::metrics::Recorder;
use crate::ports::Timestamp;

/// One lease kind as it stands: who holds it, since when, and who waits
///
/// A holder is named by whatever tells it apart from the others of its kind.
#[derive(Debug, Default)]
pub(super) struct Standing {
    pub(super) holders: Vec<(String, Timestamp)>,
    pub(super) queue: Vec<String>,
}

/// Where each kind stood the last time the desk looked, and when each waiter
/// was first seen waiting
#[derive(Debug, Default)]
pub(super) struct Watch {
    held: BTreeMap<String, BTreeSet<(String, Timestamp)>>,
    queued: BTreeMap<(String, String), Timestamp>,
}

impl Watch {
    /// Sends how `standing` stands at `now` and counts the grants since last
    /// time, unless `counting` is off
    ///
    /// A holder seated without waiting, for a command that already runs under
    /// a grant from a dog that went away, is not a grant: the dog before this
    /// one counted it.
    pub(super) fn look(
        &mut self,
        metrics: &Recorder,
        now: Timestamp,
        standing: BTreeMap<String, Standing>,
        counting: bool,
    ) {
        for (kind, now_standing) in standing {
            for who in &now_standing.queue {
                self.queued
                    .entry((kind.clone(), who.clone()))
                    .or_insert(now);
            }
            let before = self.held.remove(&kind).unwrap_or_default();
            for holder in &now_standing.holders {
                if before.contains(holder) {
                    continue;
                }
                let waited = self.queued.remove(&(kind.clone(), holder.0.clone()));
                if counting {
                    let waited = waited.map_or(0, |from| holder.1.0.saturating_sub(from.0));
                    metrics.lease_granted(&kind, waited);
                }
            }
            // A waiter that left the queue without a grant took no wait with it.
            self.queued
                .retain(|(k, who), _| *k != kind || now_standing.queue.contains(who));
            let held = now_standing.holders.len();
            metrics.lease_standing(&kind, held, now_standing.queue.len());
            self.held
                .insert(kind, now_standing.holders.into_iter().collect());
        }
    }
}

impl Desk {
    // Sends how every lease stands and counts the grants since the last look.
    pub(super) fn look(&mut self, counting: bool) {
        let mut standing = BTreeMap::<String, Standing>::new();
        let mut add = |kind: &str, holders: Vec<(String, Timestamp)>, queue: Vec<String>| {
            let line = standing.entry(kind.to_owned()).or_default();
            line.holders.extend(holders);
            line.queue.extend(queue);
        };
        add_counted(&mut add, CARGO_TEST, "", self.tests.status());
        for (host, lease) in &self.e2e {
            add_counted(&mut add, crate::lease::door::E2E, host, lease.status());
        }
        for lease in self.book.status() {
            let holders = (lease.holder.iter().zip(lease.since))
                .map(|(holder, since)| (format!("{holder:?}"), since))
                .collect();
            let queue = lease.queue.iter().map(|w| format!("{w:?}")).collect();
            add(lease.kind.as_str(), holders, queue);
        }
        let now = self.book.now();
        self.watch.look(&self.metrics, now, standing, counting);
    }
}

// A counted lease's holders and waiters as the watch names them: a ticket is
// one connection, and an e2e host's tickets are told apart by the host.
fn add_counted(
    add: &mut impl FnMut(&str, Vec<(String, Timestamp)>, Vec<String>),
    kind: &str,
    host: &str,
    status: CountedStatus,
) {
    let name = |ticket: Ticket| format!("{host}/{}", ticket.0);
    let holders = (status.holders.iter())
        .map(|h| (name(h.taker.ticket), h.since))
        .collect();
    let queue = status.queue.iter().map(|t| name(t.ticket)).collect();
    add(kind, holders, queue);
}

#[cfg(test)]
mod tests;
