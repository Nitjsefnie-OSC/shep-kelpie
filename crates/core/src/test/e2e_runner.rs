//! The rig's e2e runner: returns the outcomes a test scripts, one per run,
//! and keeps every run it was asked for

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use super::Hold;
use crate::ports::{E2eOutcome, E2eRun, E2eRunner};

/// One run the runner was asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eRan {
    /// What it was asked to run
    pub run: E2eRun,
    /// Whether its worktree held a checkout as it ran
    pub checked_out: bool,
}

/// A stand-in end-to-end runner that answers from a script
#[derive(Debug, Clone, Default)]
pub struct FakeE2e {
    outcomes: Arc<Mutex<VecDeque<E2eOutcome>>>,
    ran: Arc<Mutex<Vec<E2eRan>>>,
    holds: Arc<Mutex<VecDeque<Hold>>>,
}

impl FakeE2e {
    /// Queues the outcomes of the next runs, one each. A run past the queue passes.
    pub fn script(&self, outcomes: impl IntoIterator<Item = E2eOutcome>) {
        self.outcomes.lock().unwrap().extend(outcomes);
    }

    /// Makes the next run block until `hold` is released, after it is recorded
    pub fn hold_next(&self, hold: Hold) {
        self.holds.lock().unwrap().push_back(hold);
    }

    /// Every run asked for, oldest first
    pub fn ran(&self) -> Vec<E2eRan> {
        self.ran.lock().unwrap().clone()
    }
}

impl E2eRunner for FakeE2e {
    fn run(&self, run: &E2eRun) -> E2eOutcome {
        self.ran.lock().unwrap().push(E2eRan {
            run: run.clone(),
            checked_out: run.worktree.join(".git").exists(),
        });
        let hold = self.holds.lock().unwrap().pop_front();
        if let Some(hold) = hold {
            hold.block();
        }
        (self.outcomes.lock().unwrap().pop_front()).unwrap_or(E2eOutcome::Passed)
    }
}
