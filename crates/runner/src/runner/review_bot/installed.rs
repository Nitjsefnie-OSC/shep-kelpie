//! A bot that may not be installed on the repo
//!
//! A bot that gave no sign of its summon and has never posted a review on
//! the repo is passed over once its summon is spent, rather than at its
//! `review_wait`: nothing it does later is likely.

use super::super::Runner;
use super::super::report::Begin;
use crate::ports::Timestamp;
use crate::review_bot::Bot;
use crate::state::StateError;
use crate::work_item::BotSkipped;

impl Runner {
    // Whether the forge says `bot` has never reviewed on this repo. A forge
    // that cannot say leaves the bot to its wait.
    pub(super) fn never_reviewed(&self, bot: Bot) -> bool {
        let profile = self.profile(bot);
        let last = self
            .ports
            .forge
            .last_bot_review(&self.remote, profile.login());
        matches!(last, Ok(None))
    }

    // The pass goes on without `bot`, whose summon sent at `at` is spent.
    pub(super) fn not_installed(&mut self, bot: Bot, at: Timestamp) -> Result<Begin, StateError> {
        let number = self.number();
        self.accepted(bot, at)?;
        if let Err(reason) = self.label(bot, number, false) {
            return Ok(self.gate_failed(reason));
        }
        let reviewer = self.round_reviewer(bot);
        self.pass_over(number, BotSkipped::NotInstalled { reviewer })
    }
}
