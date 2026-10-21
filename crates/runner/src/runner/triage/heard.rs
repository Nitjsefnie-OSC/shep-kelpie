//! Hearing the author's answers while a triage waits on them
//!
//! [`Runner::hear_triage`] runs on the comments a thread read found, next to
//! the rulings' hearing in [`super::super::thread_replies`]. It looks only at
//! issues whose triage record says the wait is open, and only at what the
//! issue's own author wrote on that issue after the triage comment went up, by
//! login ignoring case. A bot's comment, kelpie's own post and anyone else's
//! words are dropped: they are not an answer.
//!
//! Whatever the author wrote is their answers, with no word to read in it: a
//! comment that opens with `github` or answers nothing the questions asked
//! still starts the second pass, which reads it against the questions and asks
//! again only what it left unsettled. The author's comments are kept in the
//! pass until the second call is made with them. Silence `triage.wait_days`
//! after the comment ends the wait as `none` does, with nothing posted. A wait
//! the prompt is on, with no questions, is not heard from at all: its author
//! answers it by rewriting the issue body. An issue kelpie has given up on
//! after five failures is not heard from either, as no other part of triage
//! tries it again until kelpie restarts.

use super::super::Runner;
use super::super::thread_replies::plain_login;
use super::ask::answers_wait;
use crate::ports::{ThreadComment, Timestamp};
use crate::state::{StateError, Triaged};

impl Runner {
    // Takes the author's comments on an issue whose triage waits, and keeps them
    // as the answers, or ends the wait when there are none for `wait_days`.
    pub(in crate::runner) fn hear_triage(
        &mut self,
        comments: &[ThreadComment],
    ) -> Result<(), StateError> {
        for record in self.triage_waiting() {
            // A call on the issue is in flight: its answer is heard as news,
            // and the wait is not read again under it. An issue kelpie has given
            // up on is not read either, as no other part of triage tries it
            // again until kelpie restarts.
            if self.flights.triaging() == Some(record.issue) || self.triage_given_up(record.issue) {
                continue;
            }
            let Some(author) = self
                .triage_tries
                .get(&record.issue)
                .and_then(|t| t.author.clone())
            else {
                continue;
            };
            // A wait on the author's own session alone is answered by the body
            // their session rewrote, not by a comment, so it is not heard from.
            let Some((_, wait_at)) = answers_wait(&record) else {
                continue;
            };
            let replies = self.their_replies(&record, &author, wait_at, comments);
            self.hear_answers(&record, wait_at, &replies)?;
        }
        Ok(())
    }

    // What `author` wrote on `issue` after `wait_at`, oldest first: their own
    // words, and no one else's
    fn their_replies<'a>(
        &self,
        record: &Triaged,
        author: &str,
        wait_at: Timestamp,
        comments: &'a [ThreadComment],
    ) -> Vec<&'a ThreadComment> {
        let app = self.app_login().map(|app| plain_login(&app));
        let mine = plain_login(author);
        let mut replies: Vec<&ThreadComment> = comments
            .iter()
            .filter(|c| {
                c.thread == record.issue
                    && !c.bot
                    && plain_login(&c.author) == mine
                    && app
                        .as_deref()
                        .is_none_or(|app| plain_login(&c.author) != app)
                    && c.created > wait_at
            })
            .collect();
        replies.sort_by_key(|c| (c.created, c.id));
        replies
    }

    // The author's comments after the triage comment are the answers: they go
    // to the pass, which starts the second triager call with them. With none,
    // silence `triage.wait_days` after the comment counts as `none`.
    fn hear_answers(
        &mut self,
        record: &Triaged,
        wait_at: Timestamp,
        replies: &[&ThreadComment],
    ) -> Result<(), StateError> {
        let issue = record.issue;
        if replies.is_empty() {
            if self.ports.clock.now().0 < wait_at.0.saturating_add(self.wait_seconds()) {
                return Ok(());
            }
            self.notes.push(format!(
                "triage #{issue}: its author gave no answers within `triage.wait_days` of the \
                 questions, so the maintainer triages it as it stands"
            ));
            return self.triage_none(issue);
        }
        let text: Vec<String> = replies.iter().map(|c| c.body.trim().to_owned()).collect();
        if let Some(tries) = self.triage_tries.get_mut(&issue) {
            tries.answers = Some(text.join("\n\n"));
        }
        Ok(())
    }
}
