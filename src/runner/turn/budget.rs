//! A work item past its spending cap
//!
//! The cap is checked before a worker turn starts, so a turn already
//! running is never interrupted. The work item parks on a `stuck` ruling for
//! `over-budget`, and an answer doubles its cap.

use crate::runner::Runner;
use crate::runner::report::{Begin, StepReport};
use crate::runner::ruling::park;
use crate::state::{StateError, Stuck};

impl Runner {
    // Parks the work item on a ruling when its priced calls cost more than
    // its cap, and reports it as a turn that could not run. The turn stays as
    // it stood, so the answer carries on with it.
    pub(super) fn park_over_budget(&mut self) -> Result<Option<Begin>, StateError> {
        let item = self.current().expect("a turn is a work item's");
        let per_item = self.settings.budget.per_item_usd;
        let Some((by_role, cap)) = item.over_budget(per_item) else {
            return Ok(None);
        };
        let (issue, pull_request) = (item.issue, item.pull_request);
        let kind = Stuck::OverBudget {
            phase: item.phase.clone(),
            spent: by_role.total(),
            cap,
            by_role,
        };
        let mut next = self.state.clone();
        let (id, question) = park(self.names(), &mut next, issue, pull_request, kind.into());
        self.save(next)?;
        let mut report = StepReport::Failed {
            issue,
            pull_request,
            id,
            question,
            comment_failed: None,
        };
        self.fill_comment_failed(&mut report);
        Ok(Some(Begin::Report(report)))
    }
}
