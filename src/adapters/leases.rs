//! The runner's side of the dog's leases, sent as notes on the shepherd
//! channel

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use shep_channel::Shepherd;

use crate::lease::LeaseKind;
use crate::lease::saved::BookFile;
use crate::lease::window::Window;
use crate::lease::wire::{Asker, AskerCarry, GrantError, WindowFact};
use crate::ports::{Leases, Timestamp};
use crate::review_bot::ReviewWindow;

/// Where a runner sends the dog its notes
///
/// A note is a name and a number for a dog, which shep publishes as
/// `channel.note`. Unlike a metric sample, shep's metrics dog never reads
/// one, so it refuses none of the lease names.
pub trait Notes: fmt::Debug + Send + Sync {
    /// Sends one note
    fn note(&self, name: String, value: f64);
}

impl Notes for Shepherd {
    fn note(&self, name: String, value: f64) {
        Shepherd::note(self, name, value);
    }
}

/// Leases asked for over this runner's shepherd channel
///
/// The dog answers with a `grant` trigger, which the runner's sheep hands
/// to [`ShepLeases::grant`]. When a window opens is read from the dog's
/// book file, which the dog saves after every change.
#[derive(Debug)]
pub struct ShepLeases {
    shepherd: Arc<dyn Notes>,
    asker: Mutex<Asker>,
    book: BookFile,
}

impl ShepLeases {
    /// This run's side, sending its notes through `shepherd`, and reading
    /// the dog's `book`
    pub fn new(shepherd: Arc<dyn Notes>, asker: Asker, book: BookFile) -> Self {
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

    /// Takes up what the dog's saved book says this run holds, as a runner
    /// that has just taken over from another image does once it takes
    /// `grant` triggers, since the dog does not retry a grant that found no
    /// handler
    pub fn reconcile(&self, sheep: &str) {
        match self.book.load() {
            Ok(Some(saved)) => self.asker().reconcile(sheep, &saved),
            Ok(None) => {}
            Err(e) => eprintln!("not reading the dog's book for the leases held, since {e}"),
        }
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
            for (name, value) in asker.notes() {
                self.shepherd.note(name, value);
            }
            return;
        }
        let (name, value) = asker.want(kind);
        self.shepherd.note(name, value);
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
            self.shepherd.note(name, value);
        }
    }

    fn window(&self, kind: &LeaseKind, fact: WindowFact, value: u64) {
        let (name, value) = self.asker().window(kind, fact, value);
        self.shepherd.note(name, value);
    }

    fn opens(&self, kind: &LeaseKind, window: ReviewWindow, now: Timestamp) -> Option<Timestamp> {
        let book = self.book.load().ok()??;
        let lease = book.leases.into_iter().find(|lease| &lease.kind == kind)?;
        let mut saved = Window::from(lease.window?);
        saved.define(window);
        saved.opens(now)
    }
}
