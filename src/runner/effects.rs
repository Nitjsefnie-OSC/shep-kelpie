//! What each option of a ruling leads to, worded from what kelpie knows
//!
//! A worker's question carries its options' effects in its own words. The
//! rulings kelpie raises itself, a merge and a stuck work item, get theirs
//! when they are raised: what the board holds behind the ruling, how the
//! pull request stands with CI and the reviewers, and what another review
//! round roughly costs, read from the work item's own calls. An option with
//! no effect worded keeps the phrase its kind always had.

use super::Runner;
use super::ruling::needs_pass;
use super::voice::phrases;
use crate::ports::{Cost, Role};
use crate::state::{Ruling, RulingKind, StateError};
use crate::work_item::WorkItem;

impl Runner {
    /// Words the effects of ruling `id` and keeps them with it
    ///
    /// # Errors
    ///
    /// [`StateError`] when they cannot be saved.
    pub(super) fn word_effects(&mut self, id: u64) -> Result<(), StateError> {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return Ok(());
        };
        let effects = self.effects_of(ruling);
        if effects.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        if let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) {
            ruling.effects = effects;
        }
        self.save(next)
    }

    // The effect of each option of `ruling`, in the order its card lists them,
    // or none where kelpie words none
    fn effects_of(&self, ruling: &Ruling) -> Vec<String> {
        let Some(item) = ruling.issue.and_then(|issue| self.state.item(issue)) else {
            return Vec::new();
        };
        let held = self.parked_note(ruling.id);
        match &ruling.kind {
            RulingKind::Merge { .. } => self.merge_effects(item, &ruling.kind, held),
            RulingKind::Stuck(_) => (phrases(&ruling.kind).into_iter())
                .map(|does| join([Some(does), held.clone()]))
                .collect(),
            _ => Vec::new(),
        }
    }

    // A merge ruling's options: the merge says what it closes, how CI and the
    // reviewers stand, and what waits on the ruling; the ways back say what
    // the next read of the pull request costs
    fn merge_effects(
        &self,
        item: &WorkItem,
        kind: &RulingKind,
        held: Option<String>,
    ) -> Vec<String> {
        let ci = self.settings.ci.block.then(|| "CI passed.".to_owned());
        let merge = join([
            Some(format!(
                "Merges it into main and closes #{}: {}.",
                item.issue, item.title
            )),
            ci,
            reviewed(item, kind),
            held,
        ]);
        let again = join([
            Some("Sends the worker back for a change the whole review reads again.".to_owned()),
            round_cost(item),
        ]);
        if needs_pass(kind) {
            return vec![merge, again];
        }
        let fix = "Sends the worker back for a fix that goes to CI and back to you, with no new \
                   review round.";
        vec![merge, fix.to_owned(), again]
    }
}

// Who read the pull request, where anyone did: the rounds that ran and the
// review bots that read it
fn reviewed(item: &WorkItem, kind: &RulingKind) -> Option<String> {
    if let RulingKind::Merge {
        unreviewed: Some(_),
        ..
    } = kind
    {
        return Some("No reviewer read it in its last review.".to_owned());
    }
    let rounds = match item.counts.review_rounds {
        0 => None,
        1 => Some("1 review round".to_owned()),
        n => Some(format!("{n} review rounds")),
    };
    let bots: Vec<String> = item.bot_reads.keys().map(ToString::to_string).collect();
    let bots = (!bots.is_empty()).then(|| bots.join(" and "));
    match (rounds, bots) {
        (None, None) => None,
        (Some(rounds), None) => Some(format!("{rounds} read it.")),
        (None, Some(bots)) => Some(format!("{bots} read it.")),
        (Some(rounds), Some(bots)) => Some(format!("{rounds} and {bots} read it.")),
    }
}

// What another review round costs, roughly: the average of the reviewer calls
// the work item has had priced, one to a round. A call whose harness reported
// no dollars is left out of the sum and the count alike. None before one was
// priced.
fn round_cost(item: &WorkItem) -> Option<String> {
    let priced = item
        .calls
        .iter()
        .filter(|call| call.role == Role::Reviewer && !call.unpriced);
    let (calls, spent) = priced.fold((0_u64, 0_u64), |(calls, spent), call| {
        (calls + 1, spent.saturating_add(call.cost.0))
    });
    if calls == 0 || spent == 0 {
        return None;
    }
    let each = Cost(spent / calls).usd();
    // Two decimals would print a real, small cost as $0.00.
    Some(if each < 0.005 {
        "Another review round costs under $0.01.".to_owned()
    } else {
        format!("Another review round costs about ${each:.2}.")
    })
}

// The sentences that are there, as one line
fn join<const N: usize>(sentences: [Option<String>; N]) -> String {
    sentences
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}
