//! Work that asks the forge or a model with the runner's lock let go, then takes it again to
//! apply the answer to the work item it was about

use std::sync::Mutex;

use super::Runner;
use super::trigger::lock;
use crate::ports::SessionId;
use crate::ticket::TicketId;
use crate::work_item::{Phase, Turn, WorkItem};

/// A work item as it stood when work off the lock began, to tell whether it stood still
///
/// A step of the loop moves a work item by its turn or its phase, and a session or pull request
/// that changed is another run of the same issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ItemKey {
    issue: TicketId,
    session: SessionId,
    turn: Turn,
    phase: Phase,
    pull_request: Option<u64>,
}

impl ItemKey {
    pub(super) fn of(item: &WorkItem) -> Self {
        Self {
            issue: item.issue,
            session: item.session.clone(),
            turn: item.turn.clone(),
            phase: item.phase.clone(),
            pull_request: item.pull_request,
        }
    }
}

/// Takes the lock and runs `apply` on the runner, with its focus on the work item `key` was
/// taken from
///
/// Returns `None`, running nothing, when the work item is gone or no longer as `key` has it.
#[track_caller]
pub(super) fn with_item<T>(
    runner: &Mutex<Runner>,
    key: &ItemKey,
    apply: impl FnOnce(&mut Runner) -> T,
) -> Option<T> {
    let mut runner = lock(runner);
    let item = runner.state.item(key.issue)?;
    if ItemKey::of(item) != *key {
        return None;
    }
    runner.focus = Some(key.issue);
    Some(apply(&mut runner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Timestamp;
    use crate::runner::trigger::lock;
    use crate::test::{Rig, ticket};

    fn seven() -> (Rig, Mutex<Runner>, ItemKey) {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        let key = {
            let held = lock(&runner);
            ItemKey::of(held.state.item(ticket(7)).expect("#7 is open"))
        };
        (rig, runner, key)
    }

    #[test]
    fn with_item_runs_the_apply_with_focus_on_the_item() {
        let (_rig, runner, key) = seven();
        lock(&runner).focus = None;

        let focused = with_item(&runner, &key, |runner| runner.focus);

        assert_eq!(focused, Some(Some(ticket(7))));
    }

    #[test]
    fn with_item_returns_none_when_the_item_moved_phase_or_is_gone() {
        let (_rig, runner, key) = seven();
        let ran = |runner: &Mutex<Runner>| with_item(runner, &key, |_| ()).is_some();
        assert!(ran(&runner), "the item as it was");

        lock(&runner).focus = Some(ticket(7));
        lock(&runner)
            .update(|item| {
                item.phase = Phase::Ci {
                    head: None,
                    since: Timestamp(0),
                }
            })
            .unwrap();
        assert!(!ran(&runner), "the item moved to another phase");

        lock(&runner).state.work_items.clear();
        assert!(!ran(&runner), "the item is gone");
    }
}
