//! The project's hooks at the points of kelpie's own work item loop
//!
//! `turn_end` is a gate, run here when a worker's turn ends. `ruling`, `ci`
//! and `done` are notices: the runner runs them, logs a non-zero exit or a
//! hang with the hook's name, and goes on. `turn_start` and `reset` run on
//! the thread of the call they come before, in [`super::turn_hooks`]. All run
//! in the runner, outside any sandbox, so they read what the runner can.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::Runner;
use super::words::Wants;
use crate::hooks::Role;
use crate::settings::HookPoint;
use crate::state::{Finished, ProjectState, Ruling};
use crate::work_item::{Phase, WorkItem};

/// What the notice points have told already, in memory only
///
/// A restart tells a head's CI once more, and no ruling or work item again:
/// the rulings it finds are taken as told already.
#[derive(Debug, Default)]
pub(super) struct Told {
    // The rulings told to the `ruling` hooks
    rulings: BTreeSet<u64>,
    // The head each work item's CI last settled on, told to the `ci` hooks
    ci: BTreeMap<u64, String>,
}

impl Told {
    /// What a runner that opens on `state` has told: every ruling it holds
    pub(super) fn of(state: &ProjectState) -> Self {
        Self {
            rulings: state.rulings.iter().map(|r| r.id).collect(),
            ci: BTreeMap::new(),
        }
    }
}

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

    /// Tells the `ruling` hooks each ruling raised since they last heard
    pub(super) fn tell_rulings(&mut self) {
        if !self.hooks.has(HookPoint::Ruling) {
            return;
        }
        let told = &mut self.told_points.rulings;
        let raised: Vec<Ruling> = (self.state.rulings.iter())
            .filter(|r| told.insert(r.id))
            .cloned()
            .collect();
        for ruling in raised {
            let kind = serde_json::to_value(&ruling.kind).unwrap_or(Value::Null);
            let choices: &[&str] = match Wants::of(&ruling.kind) {
                Wants::Answer => &[],
                Wants::Merge => &["yes", "no", "rework"],
                Wants::YesOrNo => &["yes", "no"],
            };
            let details = json!({
                "id": ruling.id,
                "issue": ruling.issue,
                "pull_request": ruling.pull_request,
                "kind": kind["kind"],
                "question": ruling.question,
                "choices": choices,
            });
            self.tell_hooks(HookPoint::Ruling, Role::Kelpie, details);
        }
    }

    /// Tells the `ci` hooks that CI settled on `head` of `issue`'s pull
    /// request `number` as `conclusion`, once for each head
    pub(super) fn ci_settled(
        &mut self,
        issue: u64,
        number: u64,
        head: &str,
        conclusion: &str,
        failed: &[String],
    ) {
        if !self.hooks.has(HookPoint::Ci) {
            return;
        }
        if self
            .told_points
            .ci
            .insert(issue, head.to_owned())
            .as_deref()
            == Some(head)
        {
            return;
        }
        let details = json!({
            "issue": issue,
            "pull_request": number,
            "head": head,
            "conclusion": conclusion,
            "failed": failed,
        });
        self.tell_hooks(HookPoint::Ci, Role::Kelpie, details);
    }

    /// Tells the `done` hooks that a work item ended, with its history entry
    pub(super) fn tell_done(&self, record: &Finished) {
        let details = json!({
            "issue": record.issue,
            "pull_request": record.pull_request,
            "merged": record.merged,
            "closed": record.closed,
            "history": record,
        });
        self.tell_hooks(HookPoint::Done, Role::Kelpie, details);
    }
}
