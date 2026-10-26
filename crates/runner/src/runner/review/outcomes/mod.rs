//! What each finding a fix turn was sent came to, kept in the journal
//!
//! Derived from what the fix turn already reads of the worker's files and
//! reply, so the worker answers nothing new. A turn that settles without a
//! ruling writes one entry; one that raises `FixNotPushed` writes none, since
//! the worker gets another turn and the entry comes then.

use super::disputes::{self, Answers};
use super::findings::Sent;
use super::{Review, checks};
use crate::ports::Finding;
use crate::runner::Runner;
use crate::runner::retro::journal::{Entry, FindingOutcome, Outcome};
use crate::state::StateError;
use crate::work_item::{ThreadAnswer, WorkItem};

// How `finding`, the `at`th sent, came out.
fn outcome_of(
    item: &WorkItem,
    answers: &Answers,
    by_thread: bool,
    at: usize,
    finding: &Finding,
    pushed: bool,
) -> (Outcome, Option<String>) {
    if by_thread {
        return match item.answer_of(&item.threads_sent[at]) {
            Some(ThreadAnswer::Fixed { .. }) if pushed => (Outcome::Fixed, None),
            Some(ThreadAnswer::Declined { reason, .. }) => {
                (Outcome::Declined, Some(reason.clone()))
            }
            _ => (Outcome::NotAddressed, None),
        };
    }
    if let Some((_, reason)) = answers.disputed().iter().find(|(i, _)| *i == at) {
        return (Outcome::Disputed, Some(reason.clone()));
    }
    if answers.repeated_at().contains(&at) {
        return (Outcome::Disputed, None);
    }
    if let Some((_, reason)) = answers.deferred().iter().find(|(i, _)| *i == at) {
        return (Outcome::Deferred, reason.clone());
    }
    match (pushed, finding.is_nit()) {
        (false, true) => (Outcome::NitDeclined, None),
        _ => (Outcome::Pushed, None),
    }
}

impl Runner {
    /// Journals what each finding `sent` came to in the fix turn, `pushed` or not
    ///
    /// Called before the bot threads sent are answered, which forgets what the
    /// worker said of them. A file that cannot be read is noted and let go, as is a
    /// journal that cannot be written. The disputed and deferred findings are counted
    /// on the work item.
    ///
    /// # Errors
    /// The state file cannot be saved.
    pub(super) fn journal_outcomes(
        &mut self,
        review: &Review,
        sent: Sent<'_>,
        pushed: bool,
    ) -> Result<(), StateError> {
        let item = self.current().expect("a fix is a work item's");
        // Only a failed try to answer the threads sent sets this, and the
        // step then runs again with some of them forgotten: the entry was
        // written on the first try.
        if item.resolve_failures > 0 {
            return Ok(());
        }
        let (issue, round) = (item.issue, review.round);
        let by_thread = item.threads_sent.len() == sent.findings.len();
        let answers = if by_thread {
            Ok(Answers::default())
        } else {
            disputes::answers(&item.build, sent, round, &item.disputes)
        };
        let answers = match answers {
            Ok(answers) => answers,
            Err(why) => {
                self.notes.push(why);
                return Ok(());
            }
        };
        let outcomes = (sent.findings.iter().enumerate())
            .filter(|(_, f)| !f.is_skipped_for_size())
            .map(|(at, f)| {
                let (outcome, reason) = outcome_of(item, &answers, by_thread, at, f, pushed);
                FindingOutcome {
                    severity: f.severity,
                    file: f.file.clone(),
                    line: f.line,
                    outcome,
                    reason,
                }
            })
            .collect::<Vec<_>>();
        if outcomes.is_empty() {
            return Ok(());
        }
        // A finding a failing test sent the worker back to was counted by the
        // round's first fix turn.
        let fresh = (sent.findings.iter())
            .filter(|f| !f.is_skipped_for_size())
            .map(|f| !checks::was_sent_back(item, round, f));
        let count = |which| {
            let outcomes = outcomes.iter().zip(fresh.clone());
            outcomes
                .filter(|(o, fresh)| *fresh && o.outcome == which)
                .count()
        };
        let disputed = u32::try_from(count(Outcome::Disputed)).unwrap_or(u32::MAX);
        let deferred = u32::try_from(count(Outcome::Deferred)).unwrap_or(u32::MAX);
        // A restart before the round leaves its fix stage settles this turn
        // again: the marker saves with the counts, so they are added once.
        let turn = item.counts.fix_turns;
        if item.counts.outcomes_counted != turn {
            self.update(|item| {
                item.counts.disputed = item.counts.disputed.saturating_add(disputed);
                item.counts.deferred = item.counts.deferred.saturating_add(deferred);
                item.counts.outcomes_counted = turn;
            })?;
        }
        let reviewer = (review.reviewer.as_ref()).map_or_else(String::new, ToString::to_string);
        self.journal(Entry::FindingOutcomes {
            issue,
            round,
            reviewer,
            outcomes,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
