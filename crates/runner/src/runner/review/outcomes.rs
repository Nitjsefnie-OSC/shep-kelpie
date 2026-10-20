//! What each finding a fix turn was sent came to, kept in the journal
//!
//! Derived from what the fix turn already reads of the worker's files and
//! reply, so the worker answers nothing new. A turn that settles without a
//! ruling writes one entry; one that raises `FixNotPushed` writes none, since
//! the worker gets another turn and the entry comes then.

use super::Review;
use super::disputes::{self, Answers};
use super::findings::Sent;
use crate::ports::Finding;
use crate::runner::Runner;
use crate::runner::retro::journal::{Entry, FindingOutcome, Outcome};
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
    if answers.deferred().contains(&at) {
        return (Outcome::Deferred, None);
    }
    match (pushed, finding.is_nit()) {
        (false, true) => (Outcome::NitDeclined, None),
        _ => (Outcome::Pushed, None),
    }
}

impl Runner {
    /// Journals what each finding `sent` came to in the fix turn that has
    /// just settled, `pushed` or not
    ///
    /// Called before the bot threads sent are answered, which forgets what
    /// the worker said of them. A file that cannot be read is noted and let
    /// go, as a journal that cannot be written is.
    pub(super) fn journal_outcomes(&mut self, review: &Review, sent: Sent<'_>, pushed: bool) {
        let item = self.current().expect("a fix is a work item's");
        // Only a failed try to answer the threads sent sets this, and the
        // step then runs again with some of them forgotten: the entry was
        // written on the first try.
        if item.resolve_failures > 0 {
            return;
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
            Err(why) => return self.notes.push(why),
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
            return;
        }
        let reviewer = (review.reviewer.as_ref()).map_or_else(String::new, ToString::to_string);
        self.journal(Entry::FindingOutcomes {
            issue,
            round,
            reviewer,
            outcomes,
        });
    }
}

#[cfg(test)]
mod tests;
