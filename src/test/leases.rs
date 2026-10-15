//! The rig's lease book: grants what is asked at once, unless a test holds
//! the grants back or hands it a real book, and keeps every ask, return of
//! a held lease and window fact in order

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::FakeClock;
use crate::adapters::{Notes, ShepLeases};
use crate::lease::book::LeaseBook;
use crate::lease::wire::{Total, TotalName, WindowFact, grant_params};
use crate::lease::{Epoch, Holder, LeaseKind};
use crate::ports::{Leases, Timestamp};
use crate::review_bot::{ReviewWindow, Reviewers};
use crate::runner::ProjectName;

/// What the runner told the dog
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Told {
    /// Asked for a kind, or asked again
    Want(LeaseKind),
    /// Gave a kind back
    Return(LeaseKind),
    /// Saw a window fact
    Window(WindowFact, u64),
}

/// The dog as the desk plays it: it acts on a total only when it is above the
/// last it saw from that run, and grants a kind that a run asks for
#[derive(Debug, Default)]
pub(crate) struct DogStandIn {
    seen: Mutex<BTreeMap<(LeaseKind, Epoch, u8), u64>>,
    grants: Mutex<Vec<(LeaseKind, Epoch)>>,
    notes: Mutex<Vec<(String, f64)>>,
}

impl DogStandIn {
    /// Whether the dog takes `epoch` to be asking for `kind`
    pub(crate) fn asking(&self, kind: &LeaseKind, epoch: Epoch) -> bool {
        let seen = self.seen.lock().unwrap();
        let total = |t: Total| seen.get(&(kind.clone(), epoch, t as u8)).copied();
        total(Total::Want) > total(Total::Return)
    }

    /// The grants the dog made since they were last taken, which it delivers
    /// to the runner they were for
    pub(crate) fn take_grants(&self) -> Vec<(LeaseKind, Epoch)> {
        std::mem::take(&mut *self.grants.lock().unwrap())
    }

    /// Every note the runner sent, oldest first
    pub(crate) fn notes(&self) -> Vec<(String, f64)> {
        self.notes.lock().unwrap().clone()
    }
}

impl Notes for DogStandIn {
    fn note(&self, name: String, value: f64) {
        self.notes.lock().unwrap().push((name.clone(), value));
        let Some(said) = TotalName::parse(&name) else {
            return;
        };
        let value = value as u64;
        let key = (said.kind.clone(), said.epoch, said.total as u8);
        let mut seen = self.seen.lock().unwrap();
        if seen.get(&key).is_some_and(|last| value <= *last) {
            return;
        }
        seen.insert(key, value);
        drop(seen);
        if said.total == Total::Want && self.asking(&said.kind, said.epoch) {
            self.grants.lock().unwrap().push((said.kind, said.epoch));
        }
    }
}

// The runner's real lease adapter over a dog stand-in.
#[derive(Debug, Clone)]
struct BehindADog {
    dog: Arc<DogStandIn>,
    leases: Arc<ShepLeases>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FakeLeases {
    behind: Arc<Mutex<Option<BehindADog>>>,
    told: Arc<Mutex<Vec<Told>>>,
    held: Arc<Mutex<BTreeSet<LeaseKind>>>,
    withheld: Arc<AtomicBool>,
    closed: Arc<Mutex<BTreeSet<LeaseKind>>>,
    opens: Arc<Mutex<BTreeMap<LeaseKind, Timestamp>>>,
    book: Arc<Mutex<Option<(LeaseBook, Holder)>>>,
}

impl FakeLeases {
    /// Answers through `leases`, the runner's real adapter, sending its
    /// notes to `dog` and taking the grants the dog makes at once
    pub(crate) fn behind_a_dog(&self, dog: Arc<DogStandIn>, leases: Arc<ShepLeases>) {
        *self.behind.lock().unwrap() = Some(BehindADog { dog, leases });
    }

    fn dog(&self) -> Option<BehindADog> {
        self.behind.lock().unwrap().clone()
    }

    /// Everything the runner told the dog, oldest first
    pub(crate) fn told(&self) -> Vec<Told> {
        self.told.lock().unwrap().clone()
    }

    /// Holds grants back, as a dog whose window is closed does, or lets
    /// the next ask through
    pub(crate) fn withhold(&self, withheld: bool) {
        self.withheld.store(withheld, Ordering::SeqCst);
    }

    /// Holds back grants of `kind` alone, as a dog whose window for it is
    /// closed does, or lets its next ask through
    pub(crate) fn close(&self, kind: &LeaseKind, closed: bool) {
        let mut all = self.closed.lock().unwrap();
        if closed {
            all.insert(kind.clone());
        } else {
            all.remove(kind);
        }
    }

    /// Has the dog's book say `kind`'s window opens at `at`, as a book whose
    /// window is spent says, until a test says otherwise
    pub(crate) fn opens_at(&self, kind: &LeaseKind, at: Timestamp) {
        self.opens.lock().unwrap().insert(kind.clone(), at);
    }

    /// Answers from a real lease book on `clock` from now on, as the runner
    /// of `project`, in a dog whose section defines `reviewers`. Each ask
    /// ticks the book, as the dog's own tick would between steps.
    pub(crate) fn use_book(&self, clock: FakeClock, project: &str, reviewers: Reviewers) {
        let mut book = LeaseBook::new(Box::new(clock));
        book.set_reviewers(reviewers);
        let me = Holder::Runner {
            project: ProjectName::try_from(project).unwrap(),
            epoch: Epoch(1),
        };
        *self.book.lock().unwrap() = Some((book, me));
    }

    /// Grants `kind` between steps, as the dog's `grant` trigger does
    pub(crate) fn grant(&self, kind: &LeaseKind) {
        self.held.lock().unwrap().insert(kind.clone());
    }

    /// Whether the runner holds `kind` now
    pub(crate) fn held(&self, kind: &LeaseKind) -> bool {
        if let Some(behind) = self.dog() {
            return behind.leases.holds(kind);
        }
        if let Some((book, me)) = self.book.lock().unwrap().as_ref() {
            return book.holder(kind) == Some(me);
        }
        self.held.lock().unwrap().contains(kind)
    }
}

impl Leases for FakeLeases {
    fn want(&self, kind: &LeaseKind) {
        self.told.lock().unwrap().push(Told::Want(kind.clone()));
        if let Some(BehindADog { dog, leases }) = self.dog() {
            leases.want(kind);
            for (kind, epoch) in dog.take_grants() {
                let _ = leases.grant(&grant_params(&kind, epoch));
            }
            return;
        }
        if let Some((book, me)) = self.book.lock().unwrap().as_mut() {
            book.ask(kind, me.clone());
            book.tick();
            return;
        }
        let closed = self.closed.lock().unwrap().contains(kind);
        if !self.withheld.load(Ordering::SeqCst) && !closed {
            self.held.lock().unwrap().insert(kind.clone());
        }
    }

    fn holds(&self, kind: &LeaseKind) -> bool {
        self.held(kind)
    }

    fn give_back(&self, kind: &LeaseKind) {
        if let Some(BehindADog { leases, .. }) = self.dog() {
            if leases.holds(kind) {
                self.told.lock().unwrap().push(Told::Return(kind.clone()));
            }
            leases.give_back(kind);
            return;
        }
        let returned = match self.book.lock().unwrap().as_mut() {
            Some((book, me)) => {
                let held = book.holder(kind) == Some(&*me);
                book.give_back(kind, me);
                held
            }
            None => self.held.lock().unwrap().remove(kind),
        };
        if returned {
            self.told.lock().unwrap().push(Told::Return(kind.clone()));
        }
    }

    fn window(&self, kind: &LeaseKind, fact: WindowFact, value: u64) {
        if let Some(BehindADog { leases, .. }) = self.dog() {
            leases.window(kind, fact, value);
        }
        if let Some((book, _)) = self.book.lock().unwrap().as_mut() {
            let at = Timestamp(value);
            let _ = match fact {
                WindowFact::Summoned => book.summoned(kind, at),
                WindowFact::Opens => book.refused(kind, at),
                WindowFact::Quota(per_hour) => book.quota(kind, per_hour, at),
            };
        }
        let told = Told::Window(fact, value);
        let mut all = self.told.lock().unwrap();
        // The runner raises a fact again each look; the dog reads it once.
        if all.last() != Some(&told) {
            all.push(told);
        }
    }

    fn opens(&self, kind: &LeaseKind, _window: ReviewWindow, now: Timestamp) -> Option<Timestamp> {
        if let Some((book, _)) = self.book.lock().unwrap().as_ref() {
            let line = book.status().into_iter().find(|l| &l.kind == kind)?;
            return line.window?.opens;
        }
        let at = self.opens.lock().unwrap().get(kind).copied()?;
        (at > now).then_some(at)
    }
}
