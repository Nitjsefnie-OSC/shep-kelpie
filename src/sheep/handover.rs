//! A runner replacing its image with the installed build, in place
//!
//! `shep_handover` execs the new build over this process, so the pid, the
//! shepherd channel's descriptor and the children all stay, and shep sees no
//! stop. A `handover` trigger asks. The loop acts at its quiet point, between
//! passes, where no merge, `git` or `gh` child of a pass is running, and holds
//! while a child's stdin is still being written, which the new image could not
//! finish. What crosses is each worker turn in flight, which the new image
//! finishes from the call's record on disk.
//!
//! The channel's descriptor is not named to `shep_handover`: the shepherd
//! passes it without close-on-exec and `SHEP_CHANNEL_FD` stays in the
//! environment, so the new image's `shep_channel::serve` takes it again.
//! Any refusal leaves this image running with every call.

use std::any::Any;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shep_handover::{Handover, State, Verdict};

use crate::adapters::ClaudeCli;
use crate::runner::{CarriedTurn, Runner};

/// The trigger that asks for a handover
pub const ACTION: &str = "handover";

/// How long a `handover` trigger waits for a refusal before it answers that
/// the request is waiting, inside the shepherd's 3s limit on an action
const ANSWER_WITHIN: Duration = Duration::from_millis(2500);

/// How often a handover held for a stdin feed looks again
pub(super) const HOLD_POLL: Duration = Duration::from_millis(200);

/// What crosses a handover: the worker turns in flight
// wire format: changing this is a change to the handover's state format
#[derive(Debug, Serialize, Deserialize)]
pub struct Carry {
    turns: Vec<CarriedTurn>,
}

impl Carry {
    pub(super) fn into_turns(self) -> Vec<CarriedTurn> {
        self.turns
    }
}

impl State for Carry {
    const FORMAT: u32 = 1;
}

/// Replaces this image with the installed build carrying `carry`, and
/// returns only why it did not
pub(super) type Exec = Box<dyn Fn(&Carry) -> String + Send + Sync>;

/// Stops calls from ending while the handover is made, or `None` when one is
/// just ending
pub(super) type Seal = Box<dyn Fn() -> Option<Box<dyn Any + Send>> + Send + Sync>;

/// The handover a `handover` trigger asked for, until the loop acts on it
pub(super) struct Handing {
    waiting: Mutex<Option<Sender<String>>>,
    exec: Exec,
    seal: Seal,
    feeding: Box<dyn Fn() -> bool + Send + Sync>,
}

impl Handing {
    /// The handover to the installed build, held while a child's stdin is
    /// written and with `claude`'s calls kept from ending while it is made
    pub(super) fn installed(claude: ClaudeCli) -> Arc<Self> {
        let seal: Seal = Box::new(move || {
            claude
                .seal()
                .map(|sealed| Box::new(sealed) as Box<dyn Any + Send>)
        });
        Self::with(
            Box::new(exec_installed),
            seal,
            Box::new(crate::adapters::feeding),
        )
    }

    /// One that no test asks for
    #[cfg(test)]
    pub(super) fn never() -> Arc<Self> {
        Self::with(
            Box::new(|_| "no handover here".into()),
            Box::new(|| Some(Box::new(()))),
            Box::new(|| false),
        )
    }

    pub(super) fn with(
        exec: Exec,
        seal: Seal,
        feeding: Box<dyn Fn() -> bool + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            waiting: Mutex::new(None),
            exec,
            seal,
            feeding,
        })
    }

    /// Whether a handover is waiting for the loop's quiet point
    pub(super) fn pending(&self) -> bool {
        self.slot().is_some()
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Sender<String>>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Answers a `handover` trigger: queues the request and wakes the loop,
    /// then reports a refusal that comes soon, or that the request waits
    ///
    /// A handover that succeeds is never answered, since the process that
    /// would answer is gone.
    pub(super) fn ask(&self, wake: &Sender<()>) -> String {
        let (tell, told) = mpsc::channel();
        {
            let mut slot = self.slot();
            if slot.is_some() {
                return error("a handover is already waiting for the runner's quiet point");
            }
            *slot = Some(tell);
        }
        let _ = wake.send(());
        match told.recv_timeout(ANSWER_WITHIN) {
            Ok(why) => error(&format!("handover refused: {why}")),
            Err(RecvTimeoutError::Timeout) => {
                serde_json::json!({ "handover": "waiting for the runner's quiet point" })
                    .to_string()
            }
            Err(RecvTimeoutError::Disconnected) => error("the handover was dropped"),
        }
    }

    /// Hands over if one is asked for and nothing holds it, at the loop's
    /// quiet point
    ///
    /// Returns only when it did not happen. A refusal is told to the
    /// trigger, if it still waits, and to the log.
    pub(super) fn attempt(&self, runner: &Arc<Mutex<Runner>>) {
        if !self.pending() || (self.feeding)() {
            return;
        }
        let Some(tell) = self.slot().take() else {
            return;
        };
        let refuse = |why: String| {
            eprintln!("handover refused: {why}");
            let _ = tell.send(why);
        };
        if let Err(e) = crate::runner::settle(runner) {
            return refuse(format!("cannot save the work items' time: {e}"));
        }
        let Some(_sealed) = (self.seal)() else {
            return refuse("a call has just ended and is being read".into());
        };
        // Held through the exec, so nothing changes what was carried.
        let held = runner.lock().unwrap_or_else(PoisonError::into_inner);
        match held.carry_calls() {
            Ok(turns) => refuse((self.exec)(&Carry { turns })),
            Err(why) => refuse(why),
        }
    }
}

fn error(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

// Execs the installed build, and says why it did not.
fn exec_installed(carry: &Carry) -> String {
    // Asked once the target is resolved and before anything is written. That
    // the target reads `Carry` is the probe's to check.
    let gate = |_: &Path| match crate::adapters::feeding() {
        true => Verdict::Refused("a child's stdin began to be written".into()),
        false => Verdict::Carryable,
    };
    match Handover::new(carry).exec(&gate) {
        Err(e) => e.to_string(),
        Ok(never) => match never {},
    }
}

#[cfg(test)]
mod tests;
