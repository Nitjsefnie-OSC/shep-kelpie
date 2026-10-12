//! A review the bot said failed
//!
//! Codex answers a summon it could not review with "Something went wrong".
//! The summon is sent once more, and the pass goes on without the bot when
//! that one fails too.

use super::super::Runner;
use super::super::report::{Begin, StepReport};
use crate::ports::Timestamp;
use crate::review_bot::Bot;
use crate::state::StateError;
use crate::work_item::{BotSkipped, Phase, ReviewStage};

impl Runner {
    /// Asks `bot` for its review once more after the first failure, and
    /// passes it over after the second
    ///
    /// The summon sent again takes `now` as its own time, so the failure that
    /// drew it is not read as its answer. Whether a failure came before is
    /// the stage's `failed`, and not `resent`, which a summon the bot gave no
    /// sign of sets too.
    ///
    /// A bot that posted a notice since the summon has had its lease given
    /// back, so the second summon asks for it again and waits for the grant.
    /// `failed` is saved before the comment goes out, as every summon is, so
    /// a restart never sends a third.
    pub(super) fn failed_review(
        &mut self,
        bot: Bot,
        (started, head): (Timestamp, String),
        now: Timestamp,
        full: bool,
    ) -> Result<Begin, StateError> {
        let number = self.number();
        if self.failed_so_far() {
            self.release(bot)?;
            if let Err(reason) = self.label(bot, number, false) {
                return Ok(self.gate_failed(reason));
            }
            let reviewer = self.round_reviewer(bot);
            return self.pass_over(number, BotSkipped::Failed { reviewer, head });
        }
        if !self.holds_own(bot) {
            if !self.lease_granted(bot) {
                return Ok(Begin::Idle);
            }
            self.hold(bot, now)?;
        }
        self.mark_failed(true)?;
        let begin = self.resend(bot, (started, head), now, full)?;
        if !matches!(begin, Begin::Report(StepReport::SummonedAgain { .. })) {
            self.mark_failed(false)?;
        }
        Ok(begin)
    }

    // Whether a failure has already drawn a second summon in this round.
    pub(super) fn failed_so_far(&self) -> bool {
        matches!(
            &self.item().phase,
            Phase::Review(review) if matches!(review.stage, ReviewStage::Summoned { failed: true, .. })
        )
    }

    fn mark_failed(&mut self, now: bool) -> Result<(), StateError> {
        self.update(|item| {
            if let Phase::Review(review) = &mut item.phase
                && let ReviewStage::Summoned { failed, .. } = &mut review.stage
            {
                *failed = now;
            }
        })
    }
}
