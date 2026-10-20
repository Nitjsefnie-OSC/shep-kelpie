//! The rig's test runner: returns the outcomes a test scripts, one per run,
//! and keeps every run it was asked for, with which watched paths the
//! worktree held as it ran

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::ports::{TestOutcome, TestRun, TestRunner};

/// One run the runner was asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// What it was asked to run
    pub run: TestRun,
    /// The watched paths the run's folder held as it ran
    pub held: Vec<PathBuf>,
}

/// A stand-in test runner that answers from a script
#[derive(Debug, Clone, Default)]
pub struct FakeTests {
    outcomes: Arc<Mutex<VecDeque<TestOutcome>>>,
    watched: Arc<Mutex<Vec<PathBuf>>>,
    left: Arc<Mutex<Vec<PathBuf>>>,
    ran: Arc<Mutex<Vec<Ran>>>,
}

impl FakeTests {
    /// Queues the outcomes of the next runs, one each. A run past the queue passes.
    pub fn script(&self, outcomes: impl IntoIterator<Item = TestOutcome>) {
        self.outcomes.lock().unwrap().extend(outcomes);
    }

    /// Notes, for every later run, whether its folder holds `path`
    pub fn watch(&self, path: &str) {
        self.watched.lock().unwrap().push(PathBuf::from(path));
    }

    /// Has every later run write a file at `path` in its folder, as a test
    /// that leaves something behind does
    pub fn leave(&self, path: &str) {
        self.left.lock().unwrap().push(PathBuf::from(path));
    }

    /// Every run asked for, oldest first
    pub fn ran(&self) -> Vec<Ran> {
        self.ran.lock().unwrap().clone()
    }
}

impl TestRunner for FakeTests {
    fn run(&self, run: &TestRun) -> TestOutcome {
        let held = (self.watched.lock().unwrap().iter())
            .filter(|path| run.cwd.join(path).exists())
            .cloned()
            .collect();
        for path in self.left.lock().unwrap().iter() {
            let at = run.cwd.join(path);
            std::fs::create_dir_all(at.parent().expect("a file has a folder")).unwrap();
            std::fs::write(at, "left behind\n").unwrap();
        }
        self.ran.lock().unwrap().push(Ran {
            run: run.clone(),
            held,
        });
        (self.outcomes.lock().unwrap().pop_front()).unwrap_or(TestOutcome::Passed)
    }
}
