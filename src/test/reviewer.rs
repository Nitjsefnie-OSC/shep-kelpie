//! The rig's local round: scripted rounds, and where its model sits

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{FakeClock, Hold};
use crate::adapters::LocalReviewer;
use crate::adapters::paddock::Timing;
use crate::ports::{Finding, Reviewer, ReviewerError, RoundStage};
use crate::settings::{Gateways, LocalRound};
use crate::worktree::Linked;

/// What the stand-in reviewer answers for its next round
#[derive(Debug, Clone)]
pub(crate) enum ScriptedRound {
    /// These findings
    Findings(Vec<Finding>),
    /// Fails with this error
    Fail(ReviewerError),
    /// Queues for the GPU until `queued` is released, then runs until
    /// `running` is released, then answers clean
    Queued { queued: Hold, running: Hold },
    /// Takes `seconds` on `clock`, then answers as `then`
    Takes {
        clock: FakeClock,
        seconds: u64,
        then: Box<ScriptedRound>,
    },
}

/// One round as the stand-in reviewer saw it
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeenRound {
    pub(crate) local: LocalRound,
    pub(crate) worktree: PathBuf,
    pub(crate) base: String,
    pub(crate) out: PathBuf,
    pub(crate) round: u32,
    pub(crate) criteria: String,
}

/// A local round's stand-in. Clean (no findings) once its script runs out, so
/// tests that do not care about the review see it pass straight through.
/// Its start check is the real one, and [`Self::pass_through`] makes its
/// rounds real too.
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeReviewer {
    seen: Arc<Mutex<Vec<SeenRound>>>,
    script: Arc<Mutex<VecDeque<ScriptedRound>>>,
    real: Arc<Mutex<Option<LocalReviewer>>>,
    unchecked: Arc<AtomicBool>,
}

impl FakeReviewer {
    /// Every round asked of it, in order
    pub(crate) fn seen(&self) -> Vec<SeenRound> {
        self.seen.lock().unwrap().clone()
    }

    /// Runs every later round with the real reviewer, after noting it
    pub(crate) fn pass_through(&self) {
        *self.real.lock().unwrap() = Some(LocalReviewer::default());
    }

    /// Runs every later round with the real reviewer, behind these gateways
    /// and with a paddock that retries at once
    pub(crate) fn pass_through_behind(&self, gateways: Gateways) {
        let quick = Timing {
            retry: Duration::from_millis(10),
            ..Timing::RUNNER
        };
        let real = LocalReviewer::default()
            .with_gateways(gateways)
            .with_paddock_timing(quick);
        *self.real.lock().unwrap() = Some(real);
    }

    /// Makes its start check pass for any round, so a test can name an endpoint
    /// that nothing answers for
    pub(crate) fn skip_check(&self) {
        self.unchecked.store(true, Ordering::SeqCst);
    }

    /// Queues answers for its next rounds, oldest first
    pub(crate) fn script(&self, rounds: impl IntoIterator<Item = ScriptedRound>) {
        self.script.lock().unwrap().extend(rounds);
    }
}

impl Reviewer for FakeReviewer {
    fn check(&self, local: &LocalRound) -> Result<(), String> {
        if self.unchecked.load(Ordering::SeqCst) {
            return Ok(());
        }
        match &*self.real.lock().unwrap() {
            Some(real) => real.check(local),
            None => LocalReviewer::default().check(local),
        }
    }

    fn round(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &Path,
        round: u32,
        criteria: &str,
    ) -> Result<Vec<Finding>, ReviewerError> {
        self.round_watched(local, worktree, base, out, round, criteria, &|_| {})
    }

    fn round_watched(
        &self,
        local: &LocalRound,
        worktree: Linked<'_>,
        base: &str,
        out: &Path,
        round: u32,
        criteria: &str,
        watch: &(dyn Fn(RoundStage) + Sync),
    ) -> Result<Vec<Finding>, ReviewerError> {
        self.seen.lock().unwrap().push(SeenRound {
            local: local.clone(),
            worktree: worktree.worktree.to_owned(),
            base: base.to_owned(),
            out: out.to_owned(),
            round,
            criteria: criteria.to_owned(),
        });
        if let Some(real) = &*self.real.lock().unwrap() {
            return real.round_watched(local, worktree, base, out, round, criteria, watch);
        }
        leave_files(out, round);
        let mut next = self.script.lock().unwrap().pop_front();
        while let Some(ScriptedRound::Takes {
            clock,
            seconds,
            then,
        }) = next
        {
            clock.advance(seconds);
            next = Some(*then);
        }
        match next {
            Some(ScriptedRound::Findings(findings)) => Ok(findings),
            Some(ScriptedRound::Fail(e)) => Err(e),
            Some(ScriptedRound::Queued { queued, running }) => {
                watch(RoundStage::Queued);
                queued.block();
                watch(RoundStage::Running);
                running.block();
                Ok(Vec::new())
            }
            Some(ScriptedRound::Takes { .. }) => unreachable!("the loop above took it"),
            None => Ok(Vec::new()),
        }
    }
}

// What an endpoint round leaves under `out`: the reply it was sent, and its report.
fn leave_files(out: &Path, round: u32) {
    let folder = out.join(format!("round-{round}"));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("reply-0.json"), "{\"scripted\":true}").unwrap();
    std::fs::write(out.join(format!("round-{round}.txt")), "scripted\n").unwrap();
}
