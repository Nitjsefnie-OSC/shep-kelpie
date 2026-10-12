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
    pub(super) fn failed_review(
        &mut self,
        bot: Bot,
        (started, head): (Timestamp, String),
        now: Timestamp,
        full: bool,
    ) -> Result<Begin, StateError> {
        let number = self.number();
        let failed_before = matches!(
            self.pass().stage,
            ReviewStage::Summoned { failed: true, .. }
        );
        if failed_before {
            self.release(bot)?;
            if let Err(reason) = self.label(bot, number, false) {
                return Ok(self.gate_failed(reason));
            }
            let reviewer = self.round_reviewer(bot);
            return self.pass_over(number, BotSkipped::Failed { reviewer, head });
        }
        let begin = self.resend(bot, (started, head), now, full)?;
        if matches!(begin, Begin::Report(StepReport::SummonedAgain { .. })) {
            self.update(|item| {
                if let Phase::Review(review) = &mut item.phase
                    && let ReviewStage::Summoned { failed, .. } = &mut review.stage
                {
                    *failed = true;
                }
            })?;
        }
        Ok(begin)
    }
}
