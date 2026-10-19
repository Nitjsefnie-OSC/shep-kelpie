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
use crate::ticket::TicketId;
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
    items: BTreeMap<TicketId, Watch>,
    free: Option<u64>,
}

impl BuildWatch {
    /// Whether the limits set the pause for `ticket`'s turn in flight, which
    /// no `undrain` may take away
    pub(super) fn holds_pause(&self, ticket: TicketId) -> bool {
        (self.items.get(&ticket)).is_some_and(|watch| watch.over && watch.told_pause)
    }

    /// The size of `ticket`'s build folder as last read
    pub(super) fn folder_bytes(&self, ticket: TicketId) -> Option<u64> {
        self.items.get(&ticket)?.reading.folder
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
        let open: Vec<TicketId> = (runner.state.work_items.iter())
            .filter_map(|item| item.ticket())
            .collect();
        runner
            .builds
            .items
            .retain(|ticket, _| open.contains(ticket));
        (
            Arc::clone(&runner.ports.disk),
            runner.settings.builds,
            runner.paths.kelpie_home.clone(),
            runner.builds_due(),
            runner.ports.clock.now(),
        )
    };
    for (ticket, build) in jobs {
        let (reading, failed) = measure(disk.as_ref(), limits, &build, &home);
        lock(runner).builds_read(ticket, now, reading, failed);
    }
}

impl Runner {
    // The items whose worker turn is in flight and is due a reading, with their
    // build folders, while any limit is on
    fn builds_due(&self) -> Vec<(TicketId, PathBuf)> {
        let limits = self.settings.builds;
        if limits.max_bytes().is_none() && limits.min_free_bytes().is_none() {
            return Vec::new();
        }
        let now = self.ports.clock.now();
        (self.state.work_items.iter())
            .filter_map(|item| Some((item.ticket()?, item.build.clone())))
            .filter(|(ticket, _)| self.flights.runs_turn(*ticket))
            .filter(|(ticket, _)| (self.builds.items.get(ticket)).is_none_or(|w| w.due(now)))
            .collect()
    }

    // Keeps a reading of `ticket`'s build folder and the disk
    fn record_build(
        &mut self,
        ticket: TicketId,
        now: Timestamp,
        reading: Reading,
        failed: Option<String>,
    ) {
        self.builds.free = reading.free.or(self.builds.free);
        let watch = self.builds.items.entry(ticket).or_default();
        watch.read_at = Some(now);
        watch.reading = reading;
        watch.unread = failed.clone();
        let Some(failed) = failed else {
            watch.told_failed = false;
            return;
        };
        if !std::mem::replace(&mut watch.told_failed, true) {
            self.notes.push(format!("{}: {failed}", ticket.shown()));
        }
    }

    // Records a reading of `ticket`'s build folder and the disk, and pauses the
    // turn in flight if it found a limit passed. A turn that ended while the
    // folder was measured has its cache pruned at once.
    fn builds_read(
        &mut self,
        ticket: TicketId,
        now: Timestamp,
        reading: Reading,
        failed: Option<String>,
    ) {
        self.record_build(ticket, now, reading, failed);
        let Some(past) = reading.past(self.settings.builds) else {
            return;
        };
        if !self.flights.runs_turn(ticket) {
            self.builds_prune(ticket);
            return;
        }
        let Some(watch) = self.builds.items.get_mut(&ticket) else {
            return;
        };
        watch.over = true;
        if watch.told_pause {
            return;
        }
        let Some(item) = (self.state.work_items.iter()).find(|item| item.ticket() == Some(ticket))
        else {
            return;
        };
        let ends = match self.pause_turn(item) {
            Pausing::AtTool => "pausing its turn at its next tool call",
            Pausing::AtEnd => "its harness has no pause hook, so its turn ends where it ends",
            // Told when a pause is set, so the next reading tries again.
            Pausing::Failed => return,
        };
        if let Some(watch) = self.builds.items.get_mut(&ticket) {
            watch.told_pause = true;
        }
        self.notes.push(format!(
            "{}: {past} ({}): {ends}, then removing the cargo cache the turn did not touch",
            ticket.shown(),
            reading.figures()
        ));
    }

    /// Marks the start of the current work item's worker turn, so a prune after
    /// it can tell what it touched, and counts the turn's first minute as read
    pub(super) fn builds_turn_begins(&mut self) {
        let Some(item) = self.current() else { return };
        let Some(ticket) = item.ticket() else { return };
        let build = item.build.clone();
        let now = self.ports.clock.now();
        self.builds.items.entry(ticket).or_default().read_at = Some(now);
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
        let Some(ticket) = self.current().and_then(|item| item.ticket()) else {
            return;
        };
        let Some(watch) = self.builds.items.get_mut(&ticket) else {
            return;
        };
        watch.told_pause = false;
        if std::mem::take(&mut watch.over) {
            self.builds_prune(ticket);
        }
    }

    // Removes the cache `ticket`'s latest turn did not touch, and holds its next
    // turn until a reading finds both limits kept. The mark of the latest
    // turn's start stays until the next turn writes its own.
    fn builds_prune(&mut self, ticket: TicketId) {
        let Some(item) = (self.state.work_items.iter()).find(|item| item.ticket() == Some(ticket))
        else {
            return;
        };
        let build = item.build.clone();
        let began = TurnMark::in_build(&build).began();
        let watch = self.builds.items.entry(ticket).or_default();
        watch.pruned = true;
        // The next turn reads again before it starts.
        watch.read_at = None;
        let note = match began {
            Some(began) => {
                let pruned = worktree::prune(&build, began);
                format!(
                    "{}: removed {} cargo cache files ({}) the turn did not touch",
                    ticket.shown(),
                    pruned.files,
                    gb(pruned.bytes)
                )
            }
            None => format!(
                "{}: cannot tell what the turn touched, so no cache was removed",
                ticket.shown()
            ),
        };
        self.notes.push(note);
    }

    // Reads the disk's free space and, with `folder`, the build folder
    fn builds_look(&mut self, ticket: TicketId, build: &Path, folder: bool) {
        let now = self.ports.clock.now();
        let limits = self.settings.builds;
        let asked = Builds {
            max_gb: if folder { limits.max_gb } else { 0 },
            ..limits
        };
        let disk = self.ports.disk.as_ref();
        let (mut reading, failed) = measure(disk, asked, build, &self.paths.kelpie_home);
        reading.folder = (reading.folder).or_else(|| {
            self.builds
                .items
                .get(&ticket)
                .and_then(|w| w.reading.folder)
        });
        self.record_build(ticket, now, reading, failed);
    }

    // Why the current work item's due turn must not start: the disk is under
    // its floor, or the item ran past a limit and a reading still finds one
    // passed, or cannot tell. Reads afresh when the last reading is a minute
    // old. The folder is read only for an item just pruned, or one not seen
    // since the runner started, since the disk is what every item shares.
    pub(super) fn builds_hold(&mut self) -> Option<Reason> {
        let item = self.current()?;
        let ticket = item.ticket()?;
        let build = item.build.clone();
        let now = self.ports.clock.now();
        let limits = self.settings.builds;
        let seen = self.builds.items.get(&ticket);
        let first = seen.is_none();
        let due = seen.is_none_or(|w| w.due(now));
        // A folder found past the cap is read again until it is not.
        let cap = limits.max_bytes();
        let over = |w: &Watch| cap.is_some_and(|cap| w.reading.folder.is_some_and(|f| f > cap));
        let with_folder = seen.is_none_or(|w| w.pruned || over(w)) && build.is_dir();
        if due {
            self.builds_look(ticket, &build, with_folder);
            // Nothing watched the turn that filled the folder, so its cache
            // is removed now.
            let found_over = self.builds.items.get(&ticket).is_some_and(over);
            if first && found_over {
                self.builds_prune(ticket);
                self.builds_look(ticket, &build, true);
            }
        }
        let watch = self.builds.items.get_mut(&ticket)?;
        let Some(why) = (watch.reading.past(limits)).or_else(|| watch.unread.clone()) else {
            watch.pruned = false;
            watch.told_hold = false;
            return None;
        };
        if !std::mem::replace(&mut watch.told_hold, true) {
            let figures = watch.reading.figures();
            self.notes.push(format!(
                "{}: {why} ({figures}): its next turn is held",
                ticket.shown()
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
            .filter_map(|item| item.ticket())
            .filter(|ticket| self.flights.runs_turn(*ticket))
            .map(|ticket| {
                let read = self.builds.items.get(&ticket).and_then(|w| w.read_at);
                let due = read.map_or(now.0, |read| read.0.saturating_add(EVERY));
                Duration::from_secs(due.saturating_sub(now.0))
            })
            .min()
    }
}
