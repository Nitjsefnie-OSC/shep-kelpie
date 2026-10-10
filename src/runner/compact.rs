//! A worker's `/compact`, sent before a turn once its session has grown
//!
//! A worker keeps one session for its whole work item, and every turn the
//! runner sends after the build resumes it and rereads all of it. Past
//! [`WORKER_COMPACT_PAST`](super::pm::WORKER_COMPACT_PAST) tokens of context
//! the runner sends `/compact` first, the way it does the project manager's.
//! The turn stays due while the compaction runs, so a compaction cut short
//! by a stop is sent again, and the turn's prompt, once it ends, opens with
//! the issue and the pull request, which the summary may have lost.

use super::Runner;
use super::report::StepReport;
use crate::ports::{AgentError, AgentReply, Role};
use crate::state::StateError;
use crate::work_item::{Turn, WorkItem};

impl Runner {
    /// Records the end of `issue`'s `/compact`
    ///
    /// A compaction that failed is let go: the turn goes on in the session
    /// as it was, and is not compacted again until its context is read anew.
    ///
    /// # Errors
    ///
    /// [`StateError`] when the work item cannot be saved.
    pub(super) fn compact_ended(
        &mut self,
        result: Result<AgentReply, AgentError>,
    ) -> Result<Option<StepReport>, StateError> {
        let issue = self.focus.expect("a compaction is a work item's");
        self.flights.landed(issue);
        // A compaction stopped with the runner is sent again on restart.
        if matches!(result, Err(AgentError::Stopped)) {
            return self.save_time().map(|()| None);
        }
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        let Some(item) = self.current_in(&mut next) else {
            return Ok(None);
        };
        let context = item.context.take();
        let report = match result {
            Ok(reply) => {
                let session = item.session.clone();
                item.record_call(
                    Role::Worker,
                    now,
                    session.clone(),
                    reply.usage,
                    reply.session_cost,
                );
                let reminder = reminder(item);
                if let Turn::Next { prompt } = &mut item.turn {
                    *prompt = format!("{reminder}\n\n{prompt}");
                }
                StepReport::WorkerCompacted {
                    issue,
                    session,
                    context,
                }
            }
            Err(e) => StepReport::WorkerCompactFailed {
                issue,
                reason: e.to_string(),
            },
        };
        self.save(next)?;
        Ok(Some(report))
    }
}

// What the worker is told ahead of its turn once its history is a summary:
// the issue and the pull request, which nothing but that history held
fn reminder(item: &WorkItem) -> String {
    let pull_request = item.pull_request.map_or_else(String::new, |number| {
        format!(
            " Your pull request #{number} is on branch `{}`.",
            item.branch
        )
    });
    format!(
        "Kelpie compacted your session, so what came before is a summary. Your work item is \
         issue #{}: {}.{pull_request} `gh issue view {}` shows the issue in full.",
        item.issue, item.title, item.issue
    )
}

#[cfg(test)]
mod tests;
