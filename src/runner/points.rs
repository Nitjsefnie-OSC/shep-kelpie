//! The project's hooks at the points of kelpie's own work item loop
//!
//! `turn_end` is a gate, run here when a worker's turn ends. `ruling`, `ci`
//! and `done` are notices: the runner runs them, logs a non-zero exit or a
//! hang with the hook's name, and goes on. `turn_start` and `reset` run on
//! the thread of the call they come before, in [`super::turn_hooks`]. All run
//! in the runner, outside any sandbox, so they read what the runner can.

use serde_json::{Value, json};

use super::Runner;
use crate::hooks::Role;
use crate::settings::HookPoint;
use crate::work_item::{Phase, WorkItem};

/// The name of `phase` as `status` spells it
pub(super) fn phase_name(phase: &Phase) -> String {
    let phase = serde_json::to_value(phase).unwrap_or(Value::Null);
    phase["state"].as_str().unwrap_or_default().to_owned()
}

impl Runner {
    /// Runs the project's `turn_end` hooks on a worker's turn that ended
    /// with `last_message`, and says why one refused it, if one did
    pub(super) fn turn_end(&self, item: &WorkItem, last_message: &str) -> Option<String> {
        if !self.hooks.has(HookPoint::TurnEnd) {
            return None;
        }
        let details = json!({
            "issue": item.issue,
            "phase": phase_name(&item.phase),
            "last_message": last_message,
            "head": item.known.head,
        });
        let checked = self.hooks.check(
            HookPoint::TurnEnd,
            Role::Worker,
            Some(&item.worktree),
            details,
        );
        checked.err().map(|refusal| refusal.to_string())
    }

    /// Runs the project's notice hooks at `point` with `details`, and logs
    /// the ones that did not exit 0
    pub(super) fn tell_hooks(&self, point: HookPoint, role: Role, details: Value) {
        if !self.hooks.has(point) {
            return;
        }
        if let Some(line) = self.hooks.notify(point, role, None, details) {
            self.notes.push(line);
        }
    }
}
