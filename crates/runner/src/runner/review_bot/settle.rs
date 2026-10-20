//! A review bot's threads, read until they settle
//!
//! The forge can list a bot's review before the threads posted with it. So
//! the round reads the threads again a little later, until two reads agree.
//! The merge ruling names any listed bot's threads nothing addressed, and
//! its nits apart.

use super::super::Runner;
use super::super::report::Begin;
use crate::ports::{Finding, Timestamp};
use crate::review_bot::{Activity, Bot, Profile, Thread};
use crate::state::StateError;
use crate::work_item::ReviewStage;

// CodeRabbit's two threads on shep#703 were missing two seconds after its
// review. Reads of the threads are this far apart.
pub(in crate::runner) const THREAD_SETTLE: u64 = 20;

// Two reads that agree land the review only once it was seen this long ago.
pub(crate) const SETTLE_LEAST: u64 = 60;

// Threads still changing this long after the review are taken as they stand.
pub(in crate::runner) const SETTLE_MOST: u64 = 120;

impl Runner {
    // A review covers the head: its threads are read again THREAD_SETTLE on.
    pub(super) fn settle_threads(
        &mut self,
        bot: Bot,
        activity: &Activity,
    ) -> Result<Begin, StateError> {
        let now = self.ports.clock.now();
        self.set_stage(ReviewStage::Settling {
            bot,
            since: now,
            read: now,
            open: open_ids(activity),
        })?;
        Ok(Begin::Idle)
    }

    // Reads the threads again, and lands the review once two reads agree
    // and SETTLE_LEAST has passed, or once SETTLE_MOST has.
    pub(super) fn threads_settling(
        &mut self,
        bot: Bot,
        since: Timestamp,
        read: Timestamp,
        open: &[String],
    ) -> Result<Begin, StateError> {
        let now = self.ports.clock.now();
        if !reread_due(now, read) {
            return Ok(Begin::Idle);
        }
        let number = self.number();
        let activity = match self.activity(bot, number) {
            Ok(activity) => activity,
            Err(reason) => return Ok(self.gate_failed(reason)),
        };
        let threads = open_ids(&activity);
        if lands(now, since, open, &threads) {
            return self.review_landed(number, bot, &activity);
        }
        self.set_stage(ReviewStage::Settling {
            bot,
            since,
            read: now,
            open: threads,
        })?;
        Ok(Begin::Idle)
    }

    /// The listed review bots' unaddressed threads on pull request
    /// `number`, as the merge ruling names them
    ///
    /// A thread is unaddressed when it is open, not outdated and not sent to
    /// a fix turn awaiting it. Nits are named apart, since a nit never holds
    /// a merge.
    ///
    /// # Errors
    ///
    /// A message when the forge cannot read a bot's activity.
    pub(in crate::runner) fn threads_open(&self, number: u64) -> Result<OpenThreads, String> {
        // A thread sent to a fix turn is the worker's, and one the
        // maintainer merged past with a yes holds nothing again.
        let passed: Vec<String> = (self.current().into_iter())
            .flat_map(|item| item.threads_sent.iter().chain(&item.threads_accepted))
            .cloned()
            .collect();
        let (mut holding, mut held_by, mut nits) = (Vec::new(), Vec::new(), Vec::new());
        for listed in self.listed_bots() {
            let profile = self.profile(listed.bot);
            let activity = self.activity(listed.bot, number)?;
            let open = activity.open_threads();
            let read = read_threads(profile.as_ref(), open.filter(|t| !passed.contains(&t.id)));
            let (low, above) = (read.nits, read.above);
            let name = profile.name();
            if !above.is_empty() {
                holding.push(format!("{} from {name}", above.len()));
                held_by.extend(above.iter().map(|(id, _)| id.clone()));
            }
            match low.len() {
                0 => {}
                1 => nits.push(format!("1 nit left open ({name})")),
                n => nits.push(format!("{n} nits left open ({name})")),
            }
        }
        let joined = |named: Vec<String>| (!named.is_empty()).then(|| named.join(", "));
        Ok(OpenThreads {
            holding: joined(holding),
            held_by,
            nits: joined(nits),
        })
    }
}

/// The listed review bots' unaddressed threads, as the merge ruling names
/// them, by bot
pub(in crate::runner) struct OpenThreads {
    /// Those above a nit, which hold a merge under `auto`, if any
    pub(in crate::runner) holding: Option<String>,
    /// The forge's ids of the threads in `holding`
    pub(in crate::runner) held_by: Vec<String>,
    /// The nits, which hold nothing, if any
    pub(in crate::runner) nits: Option<String>,
}

/// A bot's open threads read as findings: those above a nit, and the nits
/// apart, each with the forge's id of its thread
pub(in crate::runner) struct ReadThreads {
    pub(in crate::runner) above: Vec<(String, Finding)>,
    pub(in crate::runner) nits: Vec<(String, Finding)>,
}

/// Reads `threads` with the bot's `profile`, nits apart from the rest
pub(in crate::runner) fn read_threads<'a>(
    profile: &dyn Profile,
    threads: impl Iterator<Item = &'a Thread>,
) -> ReadThreads {
    let read = threads.map(|t| (t.id.clone(), profile.finding(t)));
    let (nits, above) = read.partition(|(_, finding)| finding.is_nit());
    ReadThreads { above, nits }
}

/// Whether the threads read at `read` are due to be read again at `now`
pub(in crate::runner) fn reread_due(now: Timestamp, read: Timestamp) -> bool {
    now.0.saturating_sub(read.0) >= THREAD_SETTLE
}

/// Whether the review lands at `now`: the `threads` read agree with the
/// `open` ones before them and [`SETTLE_LEAST`] has passed since the review
/// was `since` seen, or [`SETTLE_MOST`] has
pub(in crate::runner) fn lands(
    now: Timestamp,
    since: Timestamp,
    open: &[String],
    threads: &[String],
) -> bool {
    let age = now.0.saturating_sub(since.0);
    (threads == open && age >= SETTLE_LEAST) || age >= SETTLE_MOST
}

// The ids of the bot's open threads, sorted so two reads compare as sets.
pub(in crate::runner) fn open_ids(activity: &Activity) -> Vec<String> {
    let mut ids: Vec<String> = activity.open_threads().map(|t| t.id.clone()).collect();
    ids.sort();
    ids
}
