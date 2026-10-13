//! A contribution past its spending cap
//!
//! Each call's priced cost adds to the contribution's `spend`, over every head.
//! The cap is checked before a call starts, so a call already running is never
//! interrupted. Once the spend is past `budget.per_item_usd` no further call starts: a
//! `contribution` ruling for `over-budget` asks the maintainer, and an answer
//! doubles the cap, as for a work item.

use super::ruling;
use crate::runner::Runner;
use crate::state::StateError;

impl Runner {
    // Whether the contribution for `pull_request` has spent more than its cap,
    // so no call starts on it. The ruling that asks the maintainer is raised
    // once, and stays until it is answered or the contribution is gone.
    pub(super) fn over_budget(&mut self, pull_request: u64) -> Result<bool, StateError> {
        let per_item = self.settings.budget.per_item_usd;
        let over = (self.state.contributions.iter())
            .find(|c| c.pull_request == pull_request)
            .and_then(|c| c.over_budget(per_item));
        let Some(cap) = over else {
            return Ok(false);
        };
        if (self.state.rulings.iter()).any(|r| r.id == pull_request) {
            return Ok(true);
        }
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        ruling::raise_over_budget(&mut next, pull_request, cap, now);
        self.save(next)?;
        Ok(true)
    }
}
