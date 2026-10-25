//! Following the bumps with the runner's lock let go
//!
//! [`Runner::dependencies_job`] decides a poll is due and copies what it needs, [`Job::run`]
//! lists the open pull requests and reads what each bump needs from the forge and from git,
//! including the repo's up-to-date rule once any bump is behind `main`, and
//! [`Runner::apply_dependencies`] takes each bump as far as it can go on the state it finds. The
//! comments, merges and closes stay on the lock. A poll the forge refuses is read again at the
//! next pass.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use crate::board::{OpenPullRequest, is_bump};
use crate::ports::{Checks, Forge, ForgeError, PullRequestState};
use crate::runner::Runner;
use crate::runner::trigger::lock;
use crate::settings::{Dependencies, ForgeSlug};
use crate::state::{Bump, RedStep, StateError};
use crate::worktree::{self, Base, Remote, in_repo};

// The longest list of changed paths read for one bump, in bytes
const PATHS_LIMIT: usize = 1 << 20;

/// Brings the bumps the state records in line with the open pull requests and takes each as far
/// as it can go, once a poll, at the start of the pass. A listing that cannot be read leaves the
/// records as they are.
///
/// # Errors
///
/// [`StateError::Write`] when a change cannot be saved.
pub(in crate::runner) fn follow_dependencies(runner: &Mutex<Runner>) -> Result<(), StateError> {
    let job = lock(runner).dependencies_job()?;
    let Some(job) = job else {
        return Ok(());
    };
    let read = job.run();
    lock(runner).apply_dependencies(read)
}

/// What a poll read for the bumps
#[derive(Default)]
pub(super) struct Reads {
    /// Where each open bump stands on `main`, by pull request
    pub(super) proofs: BTreeMap<u64, Proof>,
    /// What the forge says of each red bump that left the board, by pull request
    pub(super) left: BTreeMap<u64, Result<PullRequestState, ForgeError>>,
    /// Whether the repo requires a pull request's branch to be up to date with its
    /// base, read once for the poll when any bump is behind `main`
    pub(super) up_to_date: Option<Result<bool, String>>,
    /// Whether a merge made the proofs read before it stale
    stale: bool,
}

impl Reads {
    // A merge moved `main`, so no proof read before it holds: the bumps still to come wait, and
    // the next pass reads them again.
    pub(super) fn moved_on(&mut self) {
        self.proofs.clear();
        self.stale = true;
    }
}

/// What git said of one open bump's head
pub(super) struct Proof {
    pub(super) base: Result<Base, String>,
    /// Whether it conflicts with `main`, read only when it is behind
    pub(super) conflicts: Option<Result<bool, String>>,
    /// Whether it changes a web path, read only when nothing else keeps it from being proved
    pub(super) web: Option<Result<bool, String>>,
}

// A poll the runner decided was due, which needs no lock
struct Job {
    forge: Arc<dyn Forge>,
    repo: ForgeSlug,
    checkout: PathBuf,
    base: String,
    dependencies: Dependencies,
    /// The bumps as the state recorded them when the poll began
    bumps: Vec<Bump>,
}

impl Job {
    fn run(self) -> Result<(Vec<OpenPullRequest>, Reads), ForgeError> {
        let open = self.forge.open_pull_requests(&self.repo)?;
        let mut reads = Reads::default();
        for pr in open.iter().filter(|pr| is_bump(&self.dependencies, pr)) {
            reads.proofs.insert(pr.number, self.proof_of(pr));
        }
        if reads
            .proofs
            .values()
            .any(|proof| matches!(proof.base, Ok(Base::Behind)))
        {
            reads.up_to_date = Some(
                self.forge
                    .requires_up_to_date(&self.repo, &self.base)
                    .map_err(|e| e.to_string()),
            );
        }
        for bump in &self.bumps {
            let number = bump.pull_request;
            let on_board = open.iter().any(|pr| pr.number == number);
            let red = bump
                .red
                .as_ref()
                .is_some_and(|red| red.step != RedStep::File);
            if !on_board && red && bump.issue_owed() {
                let read = self.forge.pull_request(&self.repo, number);
                reads.left.insert(number, read.map(|pr| pr.state));
            }
        }
        Ok((open, reads))
    }

    fn proof_of(&self, pr: &OpenPullRequest) -> Proof {
        let base = worktree::base_of(
            &self.checkout,
            &pr.head,
            &pr.commit,
            &Remote::Origin,
            &self.base,
        )
        .map_err(|e| e.to_string());
        let conflicts = matches!(base, Ok(Base::Behind))
            .then(|| merge_conflicts(&self.checkout, &pr.commit, &self.base));
        let passed = matches!(pr.checks, Checks::Passed | Checks::Skipped);
        let proved = self.bumps.iter().any(|b| {
            b.pull_request == pr.number
                && b.head == pr.commit
                && (b.e2e_failure().is_some() || b.e2e_passed())
        });
        let wanted = matches!(base, Ok(Base::Current | Base::Behind))
            && passed
            && !proved
            && !self.dependencies.web.is_empty();
        let web = wanted.then(|| {
            let paths = worktree::changed_paths(
                &self.checkout,
                pr.number,
                &pr.commit,
                PATHS_LIMIT,
                &self.base,
            );
            let web = |paths: Vec<String>| paths.iter().any(|p| self.dependencies.is_web(p));
            paths.map(web).map_err(|e| e.to_string())
        });
        Proof {
            base,
            conflicts,
            web,
        }
    }
}

// Whether merging `head` into `origin/<base>` has conflicts, which git says
// without touching the checkout. [`worktree::base_of`] has just fetched both.
fn merge_conflicts(checkout: &Path, head: &str, base: &str) -> Result<bool, String> {
    let tip = format!("origin/{base}");
    let args = ["merge-tree", "--write-tree", "--no-messages", &tip, head];
    let output = in_repo(checkout)
        .arg("--no-replace-objects")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    // Exit 1 with a tree first is a merge with conflicts. A missing commit
    // also exits 1, with nothing on stdout, and git before 2.38 fails otherwise.
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => {
            let tree = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            match tree.is_empty() || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
                true => Err(format!("git merge-tree said nothing of {head}")),
                false => Ok(true),
            }
        }
        _ => Err(format!(
            "git merge-tree failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}

impl Runner {
    // The poll, when the bumps are due one. A project that picks none forgets what the state records.
    fn dependencies_job(&mut self) -> Result<Option<Job>, StateError> {
        if self.settings.dependencies.is_off() {
            self.forget_bumps()?;
            return Ok(None);
        }
        if self
            .looks
            .dependencies_wait(self.ports.clock.now())
            .is_some()
        {
            return Ok(None);
        }
        Ok(Some(Job {
            forge: Arc::clone(&self.ports.forge),
            repo: self.remote.clone(),
            checkout: self.settings.git.checkout.clone(),
            base: self.settings.git.base().to_owned(),
            dependencies: self.settings.dependencies.clone(),
            bumps: self.state.bumps.clone(),
        }))
    }

    // Records the bumps the poll listed and takes each as far as it can go, then records the
    // poll. A listing the forge refused records the poll too, so it is asked again a poll on.
    fn apply_dependencies(
        &mut self,
        read: Result<(Vec<OpenPullRequest>, Reads), ForgeError>,
    ) -> Result<(), StateError> {
        let now = self.ports.clock.now();
        let Ok((open, mut reads)) = read else {
            self.looks.dependencies_read(now);
            return Ok(());
        };
        self.record_bumps(&open, now)?;
        let numbers: Vec<u64> = self.state.bumps.iter().map(|b| b.pull_request).collect();
        for number in numbers {
            self.work_bump(number, &open, now, &mut reads)?;
        }
        if !reads.stale {
            self.looks.dependencies_read(now);
        }
        Ok(())
    }
}
