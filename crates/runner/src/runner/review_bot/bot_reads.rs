//! A review bot's activity, which a round's step reads with the runner's lock let go
//!
//! The forge's `review_bot` read takes about two seconds. A step that finds it missing returns
//! it as a job (`turn/reads.rs`) and is made again with the answer, which stays for the step's
//! later asks. Read in place where no pass runs to make the job.

use super::super::Runner;
use super::super::report::Begin;
use crate::lease::wire::WindowFact;
use crate::ports::ForgeError;
use crate::review_bot::{Activity, Bot};

impl Runner {
    // Reads `bot`'s activity and passes the quota it last stated on.
    pub(in crate::runner) fn activity(&self, bot: Bot, number: u64) -> Result<Activity, String> {
        let profile = self.profile(bot);
        let read = (self.ports.forge).review_bot(&self.remote, number, profile.login());
        let activity = read.map_err(|e| self.cannot_read(bot, number, &e))?;
        self.note_quota(bot, &activity);
        Ok(activity)
    }

    /// `bot`'s activity as a round's step needs it: a read that failed parks the step for the
    /// gate, and a read to make is the step's `Begin::Reads`
    pub(in crate::runner) fn read_activity(
        &mut self,
        bot: Bot,
        number: u64,
    ) -> Result<Activity, Begin> {
        let read = match self.reads.activity(bot, number) {
            Some(kept) => kept.map_err(|e| self.cannot_read(bot, number, &e)),
            None => match self.activity_job(bot, number) {
                Some(job) => return Err(Begin::Reads(job)),
                None => self.activity(bot, number),
            },
        };
        read.map_err(|reason| self.gate_failed(reason))
    }

    // Passes the quota `activity` states on to the dog's book
    pub(in crate::runner) fn note_quota(&self, bot: Bot, activity: &Activity) {
        let profile = self.profile(bot);
        if let Some((per_hour, at)) = profile.quota(activity) {
            self.ports
                .leases
                .window(&profile.lease(), WindowFact::Quota(per_hour), at.0);
        }
    }

    fn cannot_read(&self, bot: Bot, number: u64, e: &ForgeError) -> String {
        format!("cannot read {} on #{number}: {e}", self.profile(bot).name())
    }
}
