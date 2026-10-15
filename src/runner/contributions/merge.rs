//! The maintainer's ruling on merging a contribution
//!
//! Nobody but the maintainer merges someone else's work, so `git.merging =
//! "auto"` does not reach a contribution. Once a poll, a contribution whose
//! pass has ended and whose CI is green on the same head gets one merge
//! ruling, about the pull request and with no work item. It names what the
//! reviewers recorded and the review bot threads still open, goes to the
//! maintainer's webhook, `status` and `shep kelpie rule` only, and is raised
//! under `ask` and under `auto` alike. It holds nothing: the clock, the asks
//! and the reviewers go on while it waits.
//!
//! The answer is kept on the record (`ruled`) and carried out by the next
//! poll, so a restart loses none of it:
//!
//! - A yes merges the head the ruling named, and the representative thanks the
//!   contributor. A forge that refuses the merge, because the head moved or
//!   for any other reason, is told in the log and asked again at the next
//!   poll. A merge that failed every try raises the ruling again. A thanks that
//!   cannot be worded or posted is told in the log and never holds the merge
//!   back.
//! - A no with a note has the representative word the note and post it for
//!   the contributor. A no with no note, or one given by a reaction or a tick,
//!   posts nothing.
//!
//! A fix turn runs for neither. A new head clears the answer, withdraws the
//! ruling and is read afresh.

use super::super::Runner;
use super::super::gate::short;
use super::ruling::{flagged_lines, raise_merge};
use crate::ports::Finding;
use crate::state::{Contribution, Held, RulingKind, StateError, Verdict};

const THANKS: &str = "Thank the contributor: their pull request is merged. Short and warm. Credit \
    them for the work, and promise nothing.";

const DECLINED: &str = "The maintainer will not merge this pull request as it stands and left a \
    note. Tell the contributor what the note says, in your own words, kindly and plainly, so they \
    know what the maintainer wants. Never promise a merge or hint that one is coming.";

impl Runner {
    // Raises the merge ruling for each contribution that is due one, and
    // carries out each answer the maintainer gave.
    pub(super) fn follow_merge(&mut self) -> Result<(), StateError> {
        let numbers: Vec<u64> = (self.state.contributions.iter())
            .map(|c| c.pull_request)
            .collect();
        for number in numbers {
            self.merge_one(number)?;
        }
        Ok(())
    }

    fn merge_one(&mut self, number: u64) -> Result<(), StateError> {
        let Some(held) = (self.state.contributions.iter())
            .find(|c| c.pull_request == number)
            .cloned()
        else {
            return Ok(());
        };
        let Some(ruled) = held.ruled.as_ref() else {
            return self.raise_for(&held);
        };
        // An answer is about the head the ruling named, never another.
        if ruled.head != held.head {
            return self.change(number, |held| held.ruled = None);
        }
        match &ruled.verdict {
            Verdict::Yes { merged, thanked } => self.merge_and_thank(&held, *merged, *thanked),
            Verdict::No { note: Some(note) } => self.tell_the_note(&held, note),
            Verdict::No { note: None } => Ok(()),
        }
    }

    // Raises the ruling when the head is due one and nothing else is asked of
    // the maintainer about the pull request
    fn raise_for(&mut self, held: &Contribution) -> Result<(), StateError> {
        let number = held.pull_request;
        if !held.awaits_merge_ruling() || (self.state.rulings.iter()).any(|r| r.id == number) {
            return Ok(());
        }
        let facts = self.merge_facts(held);
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        raise_merge(&mut next, number, &held.head, &facts, now);
        self.save(next)
    }

    // What the reviewers recorded on the head and what is still open
    fn merge_facts(&self, held: &Contribution) -> String {
        let Some(pass) = held.pass.as_ref().filter(|p| p.head == held.head) else {
            return String::new();
        };
        let all: Vec<Finding> = pass.findings.iter().chain(&pass.nits).cloned().collect();
        if all.is_empty() {
            return "The reviewers recorded no findings on this head.".to_owned();
        }
        let open = match self.open_threads(held).is_empty() {
            true => "",
            false => ", and the review bot's threads among them are still open",
        };
        format!(
            "The reviewers recorded these findings on this head{open}:\n{}",
            flagged_lines(&all)
        )
    }

    // Merges the head the ruling named, then thanks the contributor. Each
    // step is saved when it is done, so a restart does neither twice.
    fn merge_and_thank(
        &mut self,
        held: &Contribution,
        merged: bool,
        thanked: bool,
    ) -> Result<(), StateError> {
        let number = held.pull_request;
        if !merged {
            let key = format!("merge:{number}");
            if let Err(e) = self.ports.forge.merge(&self.remote, number, &held.head) {
                self.answer_failed(&key, number, "merge the pull request of", &e, true);
                if self.answer_given_up(&key) {
                    // The maintainer is asked again, with the failure in the log.
                    self.answer_ends(&key);
                    return self.change(number, |held| held.ruled = None);
                }
                return Ok(());
            }
            self.answer_ends(&key);
            self.notes.push(format!(
                "contribution #{number}: merged at {}",
                short(&held.head)
            ));
            self.change(number, |held| {
                if let Some(ruled) = held.ruled.as_mut() {
                    ruled.verdict = Verdict::Yes {
                        merged: true,
                        thanked,
                    };
                }
            })?;
        }
        if thanked {
            return Ok(());
        }
        self.thank_for_the_merge(held)
    }

    // The thanks is a courtesy: a call or a post that fails is told in the
    // log, tried at the next poll while the record is there, and nothing
    // waits on it.
    fn thank_for_the_merge(&mut self, held: &Contribution) -> Result<(), StateError> {
        let number = held.pull_request;
        let key = format!("merged:{number}");
        if self.app_covers() && !self.answer_given_up(&key) {
            let posted = match self.sent_before(&key) {
                Some(id) => Some(id),
                None => {
                    let facts = format!(
                        "Pull request #{number} by @{}. The maintainer merged it at {}.\n",
                        held.author,
                        short(&held.head)
                    );
                    self.word_and_post(held, THANKS, &facts, &key, "thank")?
                }
            };
            if posted.is_none() {
                return Ok(());
            }
            return self.keep_record(number, &key, posted, set_thanked);
        }
        self.answer_ends(&key);
        self.change(number, set_thanked)
    }

    // Has the representative word the maintainer's note for the contributor
    // and post it. A note that cannot be posted after a few tries is dropped,
    // told in the log, so the head is not held for ever.
    fn tell_the_note(&mut self, held: &Contribution, note: &str) -> Result<(), StateError> {
        let number = held.pull_request;
        let key = format!("declined:{number}");
        if self.app_covers() && !self.answer_given_up(&key) {
            let posted = match self.sent_before(&key) {
                Some(id) => Some(id),
                None => {
                    let facts = format!(
                        "Pull request #{number} by @{}, at {}. The maintainer decided not to \
                         merge it as it stands. The maintainer's note, to put to the contributor \
                         in your words:\n{note}\n",
                        held.author,
                        short(&held.head)
                    );
                    self.word_and_post(held, DECLINED, &facts, &key, "tell")?
                }
            };
            if posted.is_none() {
                return Ok(());
            }
            return self.keep_record(number, &key, posted, set_told);
        }
        self.answer_ends(&key);
        self.change(number, set_told)
    }
}

// Records that the contributor was thanked for the merge
fn set_thanked(held: &mut Contribution) {
    let Some(ruled) = held.ruled.as_mut() else {
        return;
    };
    if let Verdict::Yes { merged, .. } = ruled.verdict {
        ruled.verdict = Verdict::Yes {
            merged,
            thanked: true,
        };
    }
}

// Records that the note was put to the contributor, so it is kept no more
fn set_told(held: &mut Contribution) {
    if let Some(ruled) = held.ruled.as_mut() {
        ruled.verdict = Verdict::No { note: None };
    }
}

// Whether `kind` is the merge ruling of a contribution
pub(super) fn is_merge_ruling(kind: &RulingKind) -> bool {
    matches!(kind, RulingKind::Contribution(Held::Merge { .. }))
}
