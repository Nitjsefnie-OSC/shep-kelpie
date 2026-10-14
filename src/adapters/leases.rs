//! The runner's side of the dog's leases, raised as channel metrics

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use shep_channel::Shepherd;

use crate::lease::LeaseKind;
use crate::lease::saved::BookFile;
use crate::lease::window::Window;
use crate::lease::wire::{Asker, AskerCarry, GrantError, WindowFact};
use crate::ports::{Leases, Timestamp};
use crate::review_bot::ReviewWindow;

/// Where a runner raises its metrics
pub trait Metrics: fmt::Debug + Send + Sync {
    /// Raises one metric
    fn metric(&self, name: String, value: f64);
}

impl Metrics for Shepherd {
    fn metric(&self, name: String, value: f64) {
        Shepherd::metric(self, name, value);
    }
}

/// Leases asked for over this runner's shepherd channel
///
/// The dog answers with a `grant` trigger, which the runner's sheep hands
/// to [`ShepLeases::grant`]. When a window opens is read from the dog's
/// book file, which the dog saves after every change.
#[derive(Debug)]
pub struct ShepLeases {
    shepherd: Arc<dyn Metrics>,
    asker: Mutex<Asker>,
    book: BookFile,
}

impl ShepLeases {
    /// This run's side, raising its metrics through `shepherd`, and reading
    /// the dog's `book`
    pub fn new(shepherd: Arc<dyn Metrics>, asker: Asker, book: BookFile) -> Self {
        Self {
            shepherd,
            asker: Mutex::new(asker),
            book,
        }
    }

    /// Takes a `grant` trigger's params
    ///
    /// # Errors
    ///
    /// [`GrantError`] when the grant is malformed, for another run, or for
    /// a kind this run is not asking for.
    pub fn grant(&self, params: &str) -> Result<LeaseKind, GrantError> {
        self.asker().grant(params)
    }

    /// What the next image of this runner takes up to go on from where this
    /// one left off
    pub fn keep(&self) -> AskerCarry {
        self.asker().keep()
    }

    // Nothing holding the asker can leave it half changed.
    fn asker(&self) -> std::sync::MutexGuard<'_, Asker> {
        self.asker.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Leases for ShepLeases {
    // An ask repeated raises every total again, since the dog reads totals.
    fn want(&self, kind: &LeaseKind) {
        let mut asker = self.asker();
        if asker.asking(kind) {
            for (name, value) in asker.metrics() {
                self.shepherd.metric(name, value);
            }
            return;
        }
        let (name, value) = asker.want(kind);
        self.shepherd.metric(name, value);
    }

    fn holds(&self, kind: &LeaseKind) -> bool {
        self.asker().holds(kind)
    }

    fn keep(&self) -> Option<AskerCarry> {
        Some(ShepLeases::keep(self))
    }

    fn give_back(&self, kind: &LeaseKind) {
        let mut asker = self.asker();
        if asker.asking(kind) {
            let (name, value) = asker.give_back(kind);
            self.shepherd.metric(name, value);
        }
    }

    fn window(&self, kind: &LeaseKind, fact: WindowFact, value: u64) {
        let (name, value) = self.asker().window(kind, fact, value);
        self.shepherd.metric(name, value);
    }

    fn opens(&self, kind: &LeaseKind, window: ReviewWindow, now: Timestamp) -> Option<Timestamp> {
        let book = self.book.load().ok()??;
        let lease = book.leases.into_iter().find(|lease| &lease.kind == kind)?;
        let mut saved = Window::from(lease.window?);
        saved.define(window);
        saved.opens(now)
    }
}
