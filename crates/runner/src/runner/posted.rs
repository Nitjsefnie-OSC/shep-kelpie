//! What the loop posts for the two triggers that only ask
//!
//! `shep kelpie upgrade` and `pause` ask `status` and `drain` twice a second
//! while they wait. A pass holds the runner's lock through its forge calls,
//! and each of those waits its turn behind the login's pace, so a trigger that
//! waits on the lock waits on GitHub and answers late or not at all. The loop
//! therefore posts a copy of its status at each point where it has changed
//! what `status` shows and asked the forge nothing, and those two triggers
//! read the copy whenever the lock is held.
//!
//! A `drain` or `undrain` that lands while a pass holds the lock is answered
//! from the copy too. Its flag is set here, where every step reads it, so no
//! new call starts, and what it asks the runner itself to do (the retro over
//! what finished, the pause for each turn in flight) is kept for the pass to
//! take up at its next lock.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::{Answer, Runner};
use crate::ports::PaddockHolder;
use crate::ticket::TicketId;

/// What the loop last posted, and what a trigger asked while a pass held the lock
#[derive(Debug, Default)]
pub struct Posted {
    body: Mutex<Option<Body>>,
    pending: Mutex<Option<Pending>>,
    // Who holds each gateway model's lease, as the runner last read them, so a
    // post asks the gateway nothing
    holders: Mutex<BTreeMap<String, Option<PaddockHolder>>>,
    // Whether the runner starts no new call, which `drain` sets without the runner's lock
    draining: AtomicBool,
    // Whether a `drain` or `undrain` landed while a pass held the lock and still wants a pass
    wake: AtomicBool,
    // The verbs that read the forge and landed while a pass held the lock, oldest first
    queued: Mutex<Vec<Queued>>,
}

/// A verb that reads the forge, asked while a pass held the runner's lock, for the pass to take up
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Queued {
    /// `add <ticket>`
    Add(TicketId),
    /// `rework <pull request>`
    Rework(u64),
    /// `adopt <pull request>`
    Adopt(u64),
    /// `triage <issue>`
    Triage(u64),
    /// `rule <id> <answer>`, for a ruling the posted status lists
    Rule(TicketId, Answer),
}

/// What `status` and `drain` answer while a pass holds the runner's lock
#[derive(Debug, Clone)]
pub(crate) struct Body {
    /// The status as posted, with `draining` left out
    pub(crate) status: Value,
    /// The calls in flight and the longest any may run, whether or not the runner drains
    pub(crate) drain: Value,
}

/// What a `drain` or `undrain` asked of a runner whose lock a pass held
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    /// Hold back every new call, and with `at_tool` end each worker turn in flight at its next tool call
    Drain { at_tool: bool },
    /// Start calls again
    Undrain,
}

impl Posted {
    /// Posts `status` and the drain state, replacing what was posted
    pub(crate) fn post(&self, status: Value, drain: Value) {
        *self
            .body
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Body { status, drain });
    }

    /// What was last posted, or `None` before the runner posted anything
    pub(crate) fn body(&self) -> Option<Body> {
        self.body
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Who holds each gateway model's lease, as the runner last read them
    pub(crate) fn holders(&self) -> BTreeMap<String, Option<PaddockHolder>> {
        self.holders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Keeps `holders` as the last reading, for a post to reuse
    pub(crate) fn keep_holders(&self, holders: BTreeMap<String, Option<PaddockHolder>>) {
        *self
            .holders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = holders;
    }

    /// Whether `drain` holds back every new call
    pub(crate) fn draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Holds back every new call, or starts them again
    pub(crate) fn set_draining(&self, on: bool) {
        self.draining.store(on, Ordering::SeqCst);
    }

    /// Records a `drain` for the runner to take up at its next lock
    ///
    /// The flag is set here, so no new call starts from now, whatever the pass
    /// that holds the runner's lock goes on to read.
    ///
    /// A `drain pause` asked for once is asked for: a later plain `drain` poll
    /// that lands in the same pass takes the pause back, so the two merge into
    /// one ask that ends each turn in flight at its next tool call.
    ///
    /// Returns whether this is the first one since the runner took the last up,
    /// which is the one that asks the loop for a pass.
    pub(crate) fn ask_drain(&self, at_tool: bool) -> bool {
        self.draining.store(true, Ordering::SeqCst);
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = match *pending {
            Some(Pending::Drain { at_tool: was }) => {
                *pending = Some(Pending::Drain {
                    at_tool: was || at_tool,
                });
                false
            }
            // A `drain` after an `undrain` is the later ask, so it stands.
            Some(Pending::Undrain) => {
                *pending = Some(Pending::Drain { at_tool });
                false
            }
            None => {
                *pending = Some(Pending::Drain { at_tool });
                true
            }
        };
        drop(pending);
        // The ask a pass has not taken up yet still wants one, whatever landed
        // after it.
        self.wake.fetch_or(first, Ordering::SeqCst);
        first
    }

    /// Records an `undrain` for the runner to take up at its next lock,
    /// starting calls again from now
    pub(crate) fn ask_undrain(&self) {
        self.draining.store(false, Ordering::SeqCst);
        let first = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(Pending::Undrain)
            .is_none();
        self.wake.fetch_or(first, Ordering::SeqCst);
    }

    /// What a `drain` or `undrain` asked while a pass held the lock, if any is waiting
    ///
    /// Taking it is also what ends its ask for a pass, so a wake left over
    /// after the runner acted on it asks for no second one.
    pub(crate) fn take_pending(&self) -> Option<Pending> {
        self.wake.store(false, Ordering::SeqCst);
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// Records `verb` for the runner to take up at the top of its next pass
    ///
    /// The verb reads the forge, which a trigger must not wait on, so it
    /// answers now and the pass applies it.
    pub(crate) fn ask_queued(&self, verb: Queued) {
        self.queued
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(verb);
        self.wake.store(true, Ordering::SeqCst);
    }

    /// The verbs asked while a pass held the lock, oldest first, each handed out once
    ///
    /// Taking them also ends their ask for a pass.
    pub(crate) fn take_queued(&self) -> Vec<Queued> {
        self.wake.store(false, Ordering::SeqCst);
        std::mem::take(
            &mut *self
                .queued
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// Whether a posted `drain`, `undrain` or queued verb still wants a pass, clearing the ask
    ///
    /// A queued verb keeps asking until a pass takes it, whatever cleared the
    /// flag meanwhile.
    pub fn needs_wake(&self) -> bool {
        let waiting = !(self.queued)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty();
        self.wake.swap(false, Ordering::SeqCst) || waiting
    }
}

impl Runner {
    /// The copy of its status that `status` and `drain` read while a pass holds
    /// the runner's lock, and the drain flag they set
    pub fn postings(&self) -> Arc<Posted> {
        Arc::clone(&self.postings)
    }

    /// Whether `drain` holds back every new call
    ///
    /// Read by every step that would start a call, and set by a `drain` that
    /// landed while a pass held the runner's lock, without waiting for it.
    pub(super) fn draining(&self) -> bool {
        self.postings.draining()
    }

    /// Posts what `status` and `drain` answer, for a trigger that asks while a
    /// pass holds the runner's lock
    ///
    /// Called at each point where the runner has changed what `status` shows
    /// and asked the forge nothing, so what a trigger reads is never held up
    /// by a call that waits its turn behind the login's pace. It asks the
    /// gateway nothing either: a pass posts at each of these points, and a
    /// gateway read there would hold a trigger up behind the gateway.
    pub(super) fn post(&self) {
        let posted = self.status_of(self.posted_local_leases());
        let mut status = serde_json::to_value(posted).expect("status serializes to JSON");
        let drain = serde_json::to_value(self.drain_state()).expect("the drain serializes to JSON");
        // `draining` goes in when a trigger reads the post, from the flag the
        // post itself is not held by.
        (status.as_object_mut().expect("a status object")).remove("draining");
        self.postings.post(status, drain);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::runner::advance;
    use crate::test::{Rig, ticket};

    // A pi implementer behind a gateway, which is what makes `status` ask that
    // gateway who holds its lease.
    const BEHIND_PADDOCK: &str = "---\nrole: implementer\nharness: pi\nmodel: coder\neffort: low\n\
                                  gateway: paddock\ncontext: 65536\n---\n";

    // A post builds the whole status, so it must not ask the gateway itself:
    // a pass posts at every point where it holds the runner's lock.
    #[test]
    fn a_pass_asks_the_gateway_for_its_leases_no_more_than_status_does() {
        let rig = Rig::new("rotom");
        rig.write_agent("mine", BEHIND_PADDOCK);
        rig.implementers(&["mine"]);
        let settings = std::fs::read_to_string(rig.kelpie_settings_file()).unwrap();
        let gateway =
            "[gateways.paddock]\nurl = \"http://127.0.0.1:1/\"\nkey_env = \"CARGO_PKG_NAME\"\n";
        rig.set_kelpie_settings(&format!("{settings}{gateway}"));
        let reads = rig.count_lease_reads();
        let runner = rig.open().unwrap();
        let opened = reads.count();

        assert_eq!(rig.ask(&runner, "status", None)["project"], "rotom");
        assert_eq!(reads.count(), opened + 1, "`status` asks the gateway once");

        advance(&runner).unwrap();
        assert_eq!(reads.count(), opened + 1, "a pass asked the gateway again");
    }

    #[test]
    fn a_posted_body_is_the_last_one_posted() {
        let posted = Posted::default();
        assert!(posted.body().is_none(), "nothing posted yet");
        posted.post(json!({ "project": "koji" }), json!({ "calls": [] }));
        posted.post(json!({ "project": "koji", "run": "finishing" }), json!(0));
        let body = posted.body().expect("a post");
        assert_eq!(
            body.status,
            json!({ "project": "koji", "run": "finishing" })
        );
        assert_eq!(body.drain, json!(0));
    }

    #[test]
    fn only_the_first_drain_waiting_for_a_pass_asks_for_one() {
        let posted = Posted::default();
        assert!(posted.ask_drain(false));
        assert!(posted.needs_wake());
        assert!(!posted.needs_wake(), "the ask was already taken");
        assert!(
            !posted.ask_drain(true),
            "a second drain waits the same pass"
        );
        assert!(posted.take_pending() == Some(Pending::Drain { at_tool: true }));
        assert!(posted.take_pending().is_none());
    }

    #[test]
    fn a_drain_and_an_undrain_waiting_for_a_pass_leave_the_last_of_them() {
        let posted = Posted::default();
        posted.ask_drain(true);
        posted.ask_undrain();
        assert_eq!(posted.take_pending(), Some(Pending::Undrain));
        assert!(!posted.draining(), "the last ask is the one that stands");

        posted.ask_drain(false);
        posted.ask_drain(false);
        assert_eq!(
            posted.take_pending(),
            Some(Pending::Drain { at_tool: false })
        );
        assert!(posted.draining());
    }

    #[test]
    fn a_drain_pause_asked_once_is_asked_for_whatever_asks_after_it() {
        let posted = Posted::default();
        assert!(posted.ask_drain(true));
        assert!(
            !posted.ask_drain(false),
            "a plain `drain` poll asks for no second pass"
        );
        assert_eq!(
            posted.take_pending(),
            Some(Pending::Drain { at_tool: true }),
            "the plain poll took the pause back"
        );

        posted.ask_drain(false);
        assert!(!posted.ask_drain(true));
        assert_eq!(
            posted.take_pending(),
            Some(Pending::Drain { at_tool: true })
        );
    }

    #[test]
    fn a_second_drain_waiting_for_a_pass_keeps_the_wake_the_first_asked_for() {
        let posted = Posted::default();
        assert!(posted.ask_drain(false));
        assert!(
            !posted.ask_drain(false),
            "the first ask already wants the same pass"
        );
        assert!(
            posted.needs_wake(),
            "the second ask cleared the first's ask"
        );
        assert!(!posted.needs_wake(), "the ask was already taken");
    }

    #[test]
    fn a_queued_verb_is_taken_once_and_wakes_the_loop() {
        let posted = Posted::default();
        assert!(!posted.needs_wake(), "nothing is waiting");

        posted.ask_queued(Queued::Add(ticket(7)));
        posted.ask_queued(Queued::Adopt(71));
        assert!(posted.needs_wake(), "a queued verb asks for a pass");
        assert_eq!(
            posted.take_queued(),
            [Queued::Add(ticket(7)), Queued::Adopt(71)],
            "in the order they were asked"
        );
        assert!(posted.take_queued().is_empty(), "taken once");
        assert!(!posted.needs_wake(), "taking them ends their ask");
    }

    #[test]
    fn a_drain_taken_up_does_not_forget_a_queued_verbs_wake() {
        let posted = Posted::default();
        posted.ask_queued(Queued::Triage(9));
        posted.take_pending();
        assert!(posted.needs_wake(), "the verb still waits for its pass");
    }

    #[test]
    fn the_drain_flag_is_read_without_the_runner() {
        let posted = Posted::default();
        assert!(!posted.draining());
        posted.set_draining(true);
        assert!(posted.draining());
    }
}
