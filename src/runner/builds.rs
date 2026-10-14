//! Keeping a work item's build folder, and the disk, from filling
//!
//! Every pass while a worker turn runs measures that work item's build folder
//! and the disk that holds kelpie's home, no more than once a minute for each
//! item, and without the runner's lock. A folder past `builds.max_gb`, or a
//! disk under `builds.min_free_gb`, sets the pause for the turn, which ends at
//! its next tool call, or at its end on a harness with no hook. Once the turn
//! has ended, never before, the cargo cache it did not touch is removed. The
//! turn then resumes, unless the folder or the disk is still past a limit:
//! that holds it, and `status` says why, until a reading finds both clear.
//! All of it is in memory only: a restart reads afresh.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::Runner;
use super::drain::Pausing;
use super::trigger::lock;
use super::turn::held_up::Reason;
use crate::ports::{Disk, Timestamp};
use crate::settings::Builds;
use crate::worktree::{self, TurnMark};

/// Seconds between one reading of an item's build folder and the next
const EVERY: u64 = 60;

const GB: f64 = 1_000_000_000.0;

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GB)
}

// What one reading found
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Reading {
    // The build folder's size, when the cap is on and the folder could be read
    folder: Option<u64>,
    // The disk's free space, when the floor is on and the disk could be asked
    free: Option<u64>,
}

impl Reading {
    // Which limit this reading is past, in words, or none
    fn past(&self, limits: Builds) -> Option<String> {
        let mut past = Vec::new();
        if let (Some(max), Some(folder)) = (limits.max_bytes(), self.folder)
            && folder > max
        {
            past.push(format!(
                "its build folder holds {}, past `builds.max_gb` ({})",
                gb(folder),
                limits.max_gb
            ));
        }
        if let (Some(min), Some(free)) = (limits.min_free_bytes(), self.free)
            && free < min
        {
            past.push(format!(
                "the disk has {} free, under `builds.min_free_gb` ({})",
                gb(free),
                limits.min_free_gb
            ));
        }
        (!past.is_empty()).then(|| past.join(" and "))
    }

    // The figures, for a log line
    fn figures(&self) -> String {
        let of = |bytes: Option<u64>| bytes.map_or_else(|| "not measured".into(), gb);
        format!(
            "build folder {}, {} free on the disk",
            of(self.folder),
            of(self.free)
        )
    }
}

// Measures what `limits` turn on. The words of a failure are the second.
fn measure(
    disk: &dyn Disk,
    limits: Builds,
    build: &Path,
    home: &Path,
) -> (Reading, Option<String>) {
    let mut failed = Vec::new();
    let folder = limits.max_bytes().and_then(|_| {
        (disk.folder_bytes(build))
            .map_err(|e| failed.push(format!("cannot measure its build folder: {e}")))
            .ok()
    });
    let free = limits.min_free_bytes().and_then(|_| {
        (disk.free_bytes(home))
            .map_err(|e| failed.push(format!("cannot read the disk's free space: {e}")))
            .ok()
    });
    let failed = (!failed.is_empty()).then(|| failed.join("; "));
    (Reading { folder, free }, failed)
}

#[derive(Debug, Default)]
struct Watch {
    read_at: Option<Timestamp>,
    reading: Reading,
    // The turn in flight was found past a limit, so its cache is pruned when it ends
    over: bool,
    // Why the last reading could not measure what a limit asks for
    unread: Option<String>,
    // The turn ended past a limit and was pruned: the next turn starts only
    // once a reading finds both limits kept
    pruned: bool,
    // What the log has told since the item was last under both limits
    told_pause: bool,
    told_hold: bool,
    told_failed: bool,
}

impl Watch {
    fn due(&self, now: Timestamp) -> bool {
        self.read_at
            .is_none_or(|read| now.0.saturating_sub(read.0) >= EVERY)
    }
}

/// The work items' build folders as last read, and the disk's free space
#[derive(Debug, Default)]
pub(super) struct BuildWatch {
    items: BTreeMap<u64, Watch>,
    free: Option<u64>,
}

impl BuildWatch {
    /// The size of `issue`'s build folder as last read
    pub(super) fn folder_bytes(&self, issue: u64) -> Option<u64> {
        self.items.get(&issue)?.reading.folder
    }

    /// The disk's free space as last read
    pub(super) fn free_bytes(&self) -> Option<u64> {
        self.free
    }
}

/// Reads the build folder of each item whose turn runs and is due a reading,
/// and pauses the turn of one past a limit
///
/// The runner's lock is let go while the folders are measured.
pub(super) fn watch(runner: &Mutex<Runner>) {
    let (disk, limits, home, jobs, now) = {
        let mut runner = lock(runner);
        let open: Vec<u64> = runner.state.work_items.iter().map(|i| i.issue).collect();
        runner.builds.items.retain(|issue, _| open.contains(issue));
        (
            Arc::clone(&runner.ports.disk),
            runner.settings.builds,
            runner.paths.kelpie_home.clone(),
            runner.builds_due(),
            runner.ports.clock.now(),
        )
    };
    for (issue, build) in jobs {
        let (reading, failed) = measure(disk.as_ref(), limits, &build, &home);
        lock(runner).builds_read(issue, now, reading, failed);
    }
}

impl Runner {
    // The items whose worker turn is in flight and is due a reading, with their
    // build folders, while any limit is on
    fn builds_due(&self) -> Vec<(u64, PathBuf)> {
        let limits = self.settings.builds;
        if limits.max_bytes().is_none() && limits.min_free_bytes().is_none() {
            return Vec::new();
        }
        let now = self.ports.clock.now();
        (self.state.work_items.iter())
            .filter(|item| self.flights.runs_turn(item.issue))
            .filter(|item| (self.builds.items.get(&item.issue)).is_none_or(|watch| watch.due(now)))
            .map(|item| (item.issue, item.build.clone()))
            .collect()
    }

    // Keeps a reading of `issue`'s build folder and the disk
    fn record_build(
        &mut self,
        issue: u64,
        now: Timestamp,
        reading: Reading,
        failed: Option<String>,
    ) {
        self.builds.free = reading.free.or(self.builds.free);
        let watch = self.builds.items.entry(issue).or_default();
        watch.read_at = Some(now);
        watch.reading = reading;
        watch.unread = failed.clone();
        let Some(failed) = failed else {
            watch.told_failed = false;
            return;
        };
        if !std::mem::replace(&mut watch.told_failed, true) {
            self.notes.push(format!("issue #{issue}: {failed}"));
        }
    }

    // Records a reading of `issue`'s build folder and the disk, and pauses the
    // turn in flight if it found a limit passed. A turn that ended while the
    // folder was measured has its cache pruned at once.
    fn builds_read(
        &mut self,
        issue: u64,
        now: Timestamp,
        reading: Reading,
        failed: Option<String>,
    ) {
        self.record_build(issue, now, reading, failed);
        let Some(past) = reading.past(self.settings.builds) else {
            return;
        };
        if !self.flights.runs_turn(issue) {
            self.builds_prune(issue);
            return;
        }
        let Some(watch) = self.builds.items.get_mut(&issue) else {
            return;
        };
        watch.over = true;
        if watch.told_pause {
            return;
        }
        let Some(item) = self.state.work_items.iter().find(|i| i.issue == issue) else {
            return;
        };
        let ends = match self.pause_turn(item) {
            Pausing::AtTool => "pausing its turn at its next tool call",
            Pausing::AtEnd => "its harness has no pause hook, so its turn ends where it ends",
            // Told when a pause is set, so the next reading tries again.
            Pausing::Failed => return,
        };
        if let Some(watch) = self.builds.items.get_mut(&issue) {
            watch.told_pause = true;
        }
        self.notes.push(format!(
            "issue #{issue}: {past} ({}): {ends}, then removing the cargo cache the turn did not touch",
            reading.figures()
        ));
    }

    /// Marks the start of the current work item's worker turn, so a prune after
    /// it can tell what it touched, and counts the turn's first minute as read
    pub(super) fn builds_turn_begins(&mut self) {
        let Some(item) = self.current() else { return };
        let (issue, build) = (item.issue, item.build.clone());
        let now = self.ports.clock.now();
        self.builds.items.entry(issue).or_default().read_at = Some(now);
        if let Err(e) = TurnMark::in_build(&build).begin() {
            eprintln!(
                "cannot mark where a turn begins in {}: {e}",
                build.display()
            );
        }
    }

    // The current work item's turn has ended: removes the cache it did not
    // touch when it ran past a limit
    pub(super) fn builds_turn_ended(&mut self) {
        let Some(issue) = self.current().map(|item| item.issue) else {
            return;
        };
        let Some(watch) = self.builds.items.get_mut(&issue) else {
            return;
        };
        watch.told_pause = false;
        if std::mem::take(&mut watch.over) {
            self.builds_prune(issue);
        }
    }

    // Removes the cache `issue`'s latest turn did not touch, and holds its next
    // turn until a reading finds both limits kept. The mark of the latest
    // turn's start stays until the next turn writes its own.
    fn builds_prune(&mut self, issue: u64) {
        let Some(item) = self.state.work_items.iter().find(|i| i.issue == issue) else {
            return;
        };
        let build = item.build.clone();
        let began = TurnMark::in_build(&build).began();
        let watch = self.builds.items.entry(issue).or_default();
        watch.pruned = true;
        // The next turn reads again before it starts.
        watch.read_at = None;
        let note = match began {
            Some(began) => {
                let pruned = worktree::prune(&build, began);
                format!(
                    "issue #{issue}: removed {} cargo cache files ({}) the turn did not touch",
                    pruned.files,
                    gb(pruned.bytes)
                )
            }
            None => format!(
                "issue #{issue}: cannot tell what the turn touched, so no cache was removed"
            ),
        };
        self.notes.push(note);
    }

    // Why the current work item's due turn must not start: it ran past a limit
    // and a reading still finds one passed, or cannot tell. Reads afresh when
    // the last reading is a minute old.
    pub(super) fn builds_hold(&mut self) -> Option<Reason> {
        let item = self.current()?;
        let (issue, build) = (item.issue, item.build.clone());
        let now = self.ports.clock.now();
        let limits = self.settings.builds;
        let watch = self.builds.items.get(&issue).filter(|w| w.pruned)?;
        if watch.due(now) {
            let disk = self.ports.disk.as_ref();
            let (reading, failed) = measure(disk, limits, &build, &self.paths.kelpie_home);
            self.record_build(issue, now, reading, failed);
        }
        let watch = self.builds.items.get_mut(&issue)?;
        let Some(why) = (watch.reading.past(limits)).or_else(|| watch.unread.clone()) else {
            watch.pruned = false;
            watch.told_hold = false;
            return None;
        };
        if !std::mem::replace(&mut watch.told_hold, true) {
            let figures = watch.reading.figures();
            self.notes.push(format!(
                "issue #{issue}: {why} ({figures}): its next turn is held"
            ));
        }
        Some(Reason::Builds(why))
    }

    /// How long until the soonest reading of a build folder whose turn runs,
    /// or `None` with none due
    pub(super) fn next_build_look(&self) -> Option<Duration> {
        let now = self.ports.clock.now();
        self.builds_due_in(now)
    }

    fn builds_due_in(&self, now: Timestamp) -> Option<Duration> {
        let limits = self.settings.builds;
        if limits.max_bytes().is_none() && limits.min_free_bytes().is_none() {
            return None;
        }
        (self.state.work_items.iter())
            .filter(|item| self.flights.runs_turn(item.issue))
            .map(|item| {
                let read = self.builds.items.get(&item.issue).and_then(|w| w.read_at);
                let due = read.map_or(now.0, |read| read.0.saturating_add(EVERY));
                Duration::from_secs(due.saturating_sub(now.0))
            })
            .min()
    }
}
