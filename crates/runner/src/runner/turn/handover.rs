//! The project's `handover` hooks: a worker's finished branch, before review
//!
//! A turn that ends with the branch pushed and a pull request known hands
//! the branch over. The hooks judge it once for each head, with the commits
//! it holds past the base. A refusal goes back to the worker as its next
//! turn, once; a second parks the work item on a ruling naming the hook and
//! its reason, whose yes sends the worker back again.

use serde_json::{Value, json};

use crate::hooks::{Role, commits_past_base};
use crate::runner::Runner;
use crate::settings::HookPoint;
use crate::work_item::WorkItem;
use crate::worktree;

/// What the `handover` hooks made of a turn's branch
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Handover {
    /// No hook is due: none is set, the branch is not pushed, or this head passed already
    NotDue,
    /// Every hook passed this head
    Passed(String),
    /// A hook refused, or failed, with why
    Refused(String),
}

/// A project hook's refusal of a worker's turn: what sends the worker back
/// once, and why a second parks the work item
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refused {
    pub(super) prompt: String,
    pub(super) parked: String,
}

impl Refused {
    /// A `handover` hook refused the branch
    pub(super) fn branch(refusal: &str) -> Self {
        Self {
            prompt: format!(
                "Your branch is not ready to hand over: {refusal}. Fix it, commit and push, \
                 and end your turn. If only the maintainer can unblock you, end your reply \
                 with a <kelpie-question> block."
            ),
            parked: format!("the project's handover hook refused the branch twice: {refusal}"),
        }
    }

    /// A `turn_end` hook refused the turn
    pub(super) fn turn(refusal: &str) -> Self {
        Self {
            prompt: format!(
                "Your turn is not accepted: {refusal}. Fix it, and end your turn again. If only \
                 the maintainer can unblock you, end your reply with a <kelpie-question> block."
            ),
            parked: format!("the project's turn_end hook refused the turn twice: {refusal}"),
        }
    }

    /// Kelpie refused the title and body the worker changed in
    /// `pull-request.md`, or a `publish` hook did
    pub(super) fn pull_request_file(refusal: &str) -> Self {
        Self {
            prompt: format!(
                "Kelpie could not set the pull request's title and body from \
                 `pull-request.md`: {refusal}. Then end your turn. If only the maintainer can unblock you, end your reply with \
                 a <kelpie-question> block."
            ),
            parked: format!(
                "kelpie could not set the pull request's text from `pull-request.md`, after \
                 the worker was sent back once already: {refusal}"
            ),
        }
    }
}

impl Runner {
    /// Runs the project's `handover` hooks on `item`'s pushed head, when it
    /// has a pull request and they have not passed that head yet
    pub(super) fn handover(&self, item: &WorkItem) -> Handover {
        let Some(number) = item.pull_request else {
            return Handover::NotDue;
        };
        let Some(head) = item.known.head.clone() else {
            return Handover::NotDue;
        };
        if !self.hooks.get().has(HookPoint::Handover) || item.handed_over.as_ref() == Some(&head) {
            return Handover::NotDue;
        }
        let details = self.handover_details(item, &head, number);
        let details = match details {
            Ok(details) => details,
            // A check that cannot run must not pass silently.
            Err(e) => {
                return Handover::Refused(format!(
                    "kelpie cannot read the worktree's git to run the project's handover \
                     hook: {e}"
                ));
            }
        };
        let hooks = self.hooks.get();
        match hooks.check(
            HookPoint::Handover,
            Role::Worker,
            Some(&item.worktree),
            details,
        ) {
            Ok(()) => Handover::Passed(head),
            Err(refusal) => Handover::Refused(refusal.to_string()),
        }
    }

    // The payload: the commits the branch holds past the base, and the pull request.
    fn handover_details(&self, item: &WorkItem, head: &str, number: u64) -> Result<Value, String> {
        let run = worktree::trusted(&self.settings.git.checkout, &item.worktree)
            .map_err(|e| e.to_string())?;
        let mut details = commits_past_base(&run, head, self.item_base(item))
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| json!({ "head": head, "commits": [] }));
        details["pull_request"] = json!(number);
        details["branch"] = json!(item.branch);
        Ok(details)
    }
}
