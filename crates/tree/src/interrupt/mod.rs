//! What ends kelpie from outside, and what of it to pass on to a tree
//!
//! On Unix that is SIGINT, SIGTERM and SIGHUP. On Windows it is Ctrl+C for the
//! interrupt, Ctrl+Break for the terminate and the console closing for the
//! hangup.

use std::io;

use crate::tree::Signal;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::Source;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Source;

/// Something that ends kelpie from outside
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caught {
    /// SIGINT, or Ctrl+C
    Interrupt,
    /// SIGTERM, or Ctrl+Break
    Terminate,
    /// SIGHUP, or the console closing
    Hangup,
}

impl Caught {
    /// The shell's number for dying of it: 128 plus the signal's number
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Hangup => 129,
            Self::Interrupt => 130,
            Self::Terminate => 143,
        }
    }

    /// The signal that says the same to a tree
    #[must_use]
    pub fn signal(self) -> Signal {
        match self {
            Self::Interrupt => Signal::Interrupt,
            Self::Terminate => Signal::Terminate,
            Self::Hangup => Signal::Hangup,
        }
    }

    /// What to tell a command kelpie shares a group with, if anything
    ///
    /// A terminal or a console hands an interrupt to every process in its
    /// group, so the command has had it already.
    #[must_use]
    pub fn forward(self) -> Option<Signal> {
        (self != Self::Interrupt).then(|| self.signal())
    }

    /// What the maintainer calls it
    #[must_use]
    pub fn name(self) -> &'static str {
        match (self, cfg!(windows)) {
            (Self::Interrupt, _) => "Ctrl-C",
            (Self::Terminate, false) => "SIGTERM",
            (Self::Terminate, true) => "Ctrl-Break",
            (Self::Hangup, false) => "SIGHUP",
            (Self::Hangup, true) => "the console closing",
        }
    }
}

/// Listening for some of the [`Caught`]
///
/// Listening replaces the default of ending kelpie at once, so what kelpie
/// holds can be given back first. A kind not listened for keeps its default.
#[derive(Debug)]
pub struct Interrupts {
    interrupt: Option<Source>,
    terminate: Option<Source>,
    hangup: Option<Source>,
}

impl Interrupts {
    /// Starts listening for each of `kinds`
    ///
    /// # Errors
    ///
    /// The system refused to listen for one of them.
    pub fn listen(kinds: &[Caught]) -> io::Result<Self> {
        let source = |kind| {
            kinds
                .contains(&kind)
                .then(|| Source::listen(kind))
                .transpose()
        };
        Ok(Self {
            interrupt: source(Caught::Interrupt)?,
            terminate: source(Caught::Terminate)?,
            hangup: source(Caught::Hangup)?,
        })
    }

    /// The next one caught, never returning if none is listened for
    pub async fn recv(&mut self) -> Caught {
        tokio::select! {
            () = next(&mut self.interrupt) => Caught::Interrupt,
            () = next(&mut self.terminate) => Caught::Terminate,
            () = next(&mut self.hangup) => Caught::Hangup,
        }
    }

    /// One that has already come, if one has
    ///
    /// The system may have handed kelpie a signal that the listeners have not
    /// been told of yet, since tokio flags it from its driver, not from the
    /// handler. One yield lets the driver catch up before they are asked.
    pub async fn already(&mut self) -> Option<Caught> {
        tokio::task::yield_now().await;
        tokio::select! {
            biased;
            caught = self.recv() => Some(caught),
            () = std::future::ready(()) => None,
        }
    }
}

async fn next(source: &mut Option<Source>) {
    match source {
        Some(source) => source.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
