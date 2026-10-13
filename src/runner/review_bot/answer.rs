//! Replying on the threads a fix was sent for
//!
//! Kelpie, not the worker, writes to the pull request: it replies on each
//! thread with what the worker's reply said of it, and resolves the ones the
//! worker fixed.

use super::super::Runner;
use super::super::talk::COMMENT_REQUEST;
use crate::ports::ForgeError;
use crate::state::StateError;
use crate::work_item::{ThreadAnswer, WorkItem};

// Steps in a row that may fail to resolve the threads sent before the
// review goes on with them open: one the forge keeps refusing never resolves.
const RESOLVE_FAILURES: u32 = 3;

// Replaces a decline whose reason the forge guard refuses, as it names
// something of this machine's.
const UNPOSTABLE_REASON: &str = "Declined: the worker's reason names something on the machine kelpie runs on, so it is not posted.";

// Replaces a decline whose reason a `publish` hook of the project's refuses.
const HOOK_REFUSED_REASON: &str =
    "Declined: the worker's reason was refused by a hook of the project's, so it is not posted.";

const FIXED: &str = "Fixed.";

const NOT_FIXED: &str = "Not fixed: the worker said it was, but pushed nothing, so it stays open.";

const NOT_ADDRESSED: &str =
    "Not addressed: the worker's reply did not mention this thread, so it stays open.";

const DECLINED: &str = "Declined: ";

/// Whether `body` is a reply kelpie posts on a thread, which `gh` posts under
/// the maintainer's own login
///
/// A decline carries the worker's reason, so only its opening is known.
pub(in crate::runner) fn is_reply(body: &str) -> bool {
    [FIXED, NOT_FIXED, NOT_ADDRESSED].contains(&body) || body.starts_with(DECLINED)
}

// What kelpie replies on a thread sent to the worker, and whether it
// resolves the thread, from what the worker's reply said of it.
fn reply_to(answer: Option<&ThreadAnswer>, pushed: bool) -> (bool, String) {
    match answer {
        Some(ThreadAnswer::Fixed { .. }) if pushed => (true, FIXED.to_owned()),
        Some(ThreadAnswer::Fixed { .. }) => (false, NOT_FIXED.to_owned()),
        Some(ThreadAnswer::Declined { reason, .. }) => (false, format!("{DECLINED}{reason}")),
        None => (false, NOT_ADDRESSED.to_owned()),
    }
}

/// What replying on the threads a fix was sent came to
pub(in crate::runner) enum Answered {
    /// Every thread sent is answered, or none was sent
    Done,
    /// The forge refused for this reason, so the step is tried again
    Retry(String),
    /// The forge kept refusing, so the review goes on with these open
    LeftOpen {
        /// The forge's ids of the threads still open
        threads: Vec<String>,
        /// Why the last try failed
        reason: String,
    },
}

impl Runner {
    /// Replies on each bot thread sent to the worker, with what its reply
    /// said of the thread, and resolves the ones it fixed, on pull request
    /// `number`
    ///
    /// A thread the worker's reply did not mention is replied to as not
    /// addressed. When `pushed` is false the fix moved no head, so no thread
    /// is resolved, and a fix the worker claims is replied to as not made.
    /// Declined and unaddressed threads stay open.
    ///
    /// A thread is resolved before it is replied to, and forgotten once both
    /// are done, so a failure part way retries only what is left and a
    /// running runner never replies twice. A runner killed between a reply
    /// and the save that forgets the thread replies to it again after the
    /// restart: that order is kept, since the other would leave a thread with
    /// no reply at all.
    pub(in crate::runner) fn answer_sent(
        &mut self,
        number: u64,
        pushed: bool,
    ) -> Result<Answered, StateError> {
        if self.item().threads_sent.is_empty() {
            return Ok(Answered::Done);
        }
        let mut failed = None;
        let app = self.app_covers();
        for id in self.item().threads_sent.clone() {
            // A comment on the conversation has no thread to reply on.
            if id.starts_with(COMMENT_REQUEST) {
                self.update(|item| item.thread_answered(&id))?;
                continue;
            }
            let answer = self.item().answer_of(&id).cloned();
            let (resolve, body) = reply_to(answer.as_ref(), pushed);
            let (forge, voice) = (&self.ports.forge, &self.ports.voice);
            let resolved = match (resolve, app) {
                (false, _) => Ok(()),
                (true, true) => voice.resolve_thread(&self.remote, &id),
                (true, false) => forge.resolve_thread(&self.remote, &id),
            };
            if let Err(e) = resolved {
                failed.get_or_insert(format!("cannot resolve a thread on #{number}: {e}"));
                continue;
            }
            let reply = |text: &str| match app {
                true => voice.reply_to_thread(&self.remote, &id, text),
                false => forge.reply_to_thread(&self.remote, &id, text),
            };
            let mut posted = reply(&body);
            // The guard refuses the same text every time, so a reason that
            // names this machine, or that a hook refuses, is left out rather
            // than retried for ever. A hook that failed to run is retried.
            // Only a decline carries the worker's words, so only it is
            // replaced, by a reply that says it was a decline.
            let declined = matches!(answer, Some(ThreadAnswer::Declined { .. }));
            match &posted {
                Err(ForgeError::LocalPath { .. }) => posted = reply(UNPOSTABLE_REASON),
                Err(ForgeError::Hook { refusal, .. }) if declined && !refusal.failed => {
                    posted = reply(HOOK_REFUSED_REASON);
                }
                _ => {}
            }
            if let Err(e) = posted {
                failed.get_or_insert(format!("cannot reply to a thread on #{number}: {e}"));
                continue;
            }
            self.update(|item| item.thread_answered(&id))?;
        }
        let left = self.item().threads_sent.clone();
        let failures = self.item().resolve_failures + 1;
        match failed {
            Some(reason) if failures < RESOLVE_FAILURES => {
                self.update(|item| item.resolve_failures = failures)?;
                Ok(Answered::Retry(reason))
            }
            // Open still, they go to the worker again with the bot's next review.
            Some(reason) => {
                self.update(WorkItem::forget_threads)?;
                Ok(Answered::LeftOpen {
                    threads: left,
                    reason,
                })
            }
            None => {
                self.update(WorkItem::forget_threads)?;
                Ok(Answered::Done)
            }
        }
    }
}
