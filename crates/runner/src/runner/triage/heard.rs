//! Hearing the author's word while a triage waits on it
//!
//! [`Runner::hear_triage`] runs on the comments a thread read found, next to
//! the rulings' hearing in [`super::super::thread_replies`]. It looks only at
//! issues whose triage record says the wait is open, and only at what the
//! issue's own author wrote on that issue after the ask or the questions went
//! up, by login ignoring case. A bot's comment, kelpie's own post and anyone
//! else's words are dropped: they are not an answer.
//!
//! The reply is read as one of the words the ask gave it, `github`, `local` or
//! `none`, or `other`, by one representative call through
//! [`crate::runner::representative::reading`], which fences the reply as data.
//! A word kelpie has already read for a comment is kept in the pass, so a post
//! that failed is not paid for twice.
//!
//! `github` asks for the questions, `local` posts the prompt the author runs in
//! their own terminal ([`super::local`]), `none` ends the wait as `none`, and a
//! reply that is none of them gets one plain ask again ([`Runner::ask_again`]);
//! a second such reply counts as `none`. A wait the prompt is on is not heard
//! from at all: its author answers it by rewriting the issue body. Each reply
//! the read found is read,
//! oldest first, and the first word kelpie can read is what it acts on, so an
//! author who wrote a question and then `github` before the next read is not
//! asked again for a word they already gave. Silence `triage.wait_days` after
//! the ask, or after the questions, ends the wait the same way `none` does. An
//! issue kelpie has given up on after five failures is not heard from either,
//! as no other part of triage tries it again until kelpie restarts.

use super::super::Runner;
use super::super::representative::IssuePost;
use super::super::representative::reading::{self, Read};
use super::super::thread_replies::plain_login;
use crate::ports::{ThreadComment, Timestamp};
use crate::state::{StateError, Triaged};

/// The words a reply to the triage ask is read as, beside `other`
const WORDS: [&str; 3] = ["github", "local", "none"];

// What the representative is asked to do with a reply to the triage ask
fn read_kind() -> String {
    reading::kind(
        "The author of this issue replied to kelpie's ask of how they want its open questions \
         settled. Use github only when they clearly want kelpie to ask them on GitHub, local \
         only when they clearly want to settle them in their own terminal with kelpie's prompt, \
         none only when they clearly want the maintainer to triage the issue as it stands, and \
         other for anything else, a question included.",
        &WORDS,
    )
}

impl Runner {
    // Takes the author's comments on an issue whose triage waits, and does what
    // each one says: the questions, the plain ask again, or the end of the wait.
    pub(in crate::runner) fn hear_triage(
        &mut self,
        comments: &[ThreadComment],
    ) -> Result<(), StateError> {
        for record in self.triage_waiting() {
            // A call on the issue is in flight: its answer is heard as news,
            // and the wait is not read again under it. An issue kelpie has given
            // up on is not read either, as no other part of triage tries it
            // again until kelpie restarts: hearing it would post its ask or its
            // questions over and over.
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
            let wait_at = record
                .questions
                .map(|q| q.at)
                .or_else(|| record.asked.map(|a| a.at));
            // A wait on the author's own session is answered by the body their
            // session rewrote, not by a comment, so it is not heard from.
            let Some(wait_at) = wait_at else {
                continue;
            };
            let replies = self.their_replies(&record, &author, wait_at, comments);
            match (record.asked, record.questions) {
                (Some(asked), _) => self.hear_ask(&record, &author, asked, &replies)?,
                (None, Some(questions)) => self.hear_answers(&record, questions, &replies)?,
                (None, None) => {}
            }
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

    // Each reply to the ask, oldest first, read as `github`, `local`, `none` or
    // none of them. The first word kelpie can read is what it acts on: an author
    // who wrote a question and then `github` before the next read is not asked
    // again for a word they already gave. Two replies it cannot read count as
    // `none`.
    fn hear_ask(
        &mut self,
        record: &Triaged,
        author: &str,
        asked: crate::state::TriageAsked,
        replies: &[&ThreadComment],
    ) -> Result<(), StateError> {
        let issue = record.issue;
        let mut unread: Option<&ThreadComment> = None;
        let mut others = 0usize;
        for reply in replies {
            let Some(read) = self.read_triage_reply(issue, author, record.comment, reply)? else {
                // The reading failed and is tried at the next poll, from the
                // oldest reply again.
                return Ok(());
            };
            match read {
                Read::Word("github") => {
                    let open = self
                        .triage_tries
                        .get(&issue)
                        .is_some_and(|t| !t.open.is_empty());
                    if open {
                        self.post_questions(issue, &reply.body)?;
                    } else {
                        // A restart lost the questions the pass had: they are
                        // researched again before anything is posted.
                        if let Some(tries) = self.triage_tries.get_mut(&issue) {
                            tries.questions_call = true;
                        }
                    }
                    return Ok(());
                }
                Read::Word("local") => {
                    let open = self
                        .triage_tries
                        .get(&issue)
                        .is_some_and(|t| !t.open.is_empty());
                    if open {
                        self.post_local_prompt(issue, &reply.body)?;
                    } else {
                        // A restart lost what the prompt names: it is researched
                        // again before anything is posted.
                        if let Some(tries) = self.triage_tries.get_mut(&issue) {
                            tries.local_call = true;
                        }
                    }
                    return Ok(());
                }
                Read::Word("none") => {
                    self.notes.push(format!(
                        "triage #{issue}: its author replied `none`, so the maintainer triages it \
                         as it stands"
                    ));
                    return self.triage_none(issue);
                }
                _ => {
                    others += 1;
                    unread.get_or_insert(reply);
                }
            }
        }
        let Some(unread) = unread else {
            return self.triage_ask_silent(record, asked.at);
        };
        if asked.asked_again() || others > 1 {
            self.notes.push(format!(
                "triage #{issue}: its author answered twice in words kelpie could not read as \
                 `github`, `local` or `none`, which counts as `none`: the maintainer triages it \
                 as it stands"
            ));
            return self.triage_none(issue);
        }
        self.triage_ask_again(issue, &unread.body)
    }

    // Silence `triage.wait_days` after the ask counts as `none`
    fn triage_ask_silent(&mut self, record: &Triaged, at: Timestamp) -> Result<(), StateError> {
        if self.ports.clock.now().0 < at.0.saturating_add(self.wait_seconds()) {
            return Ok(());
        }
        self.notes.push(format!(
            "triage #{}: its author gave no word within `triage.wait_days` of the ask, which \
             counts as `none`: the maintainer triages it as it stands",
            record.issue
        ));
        self.triage_none(record.issue)
    }

    // The author's comments after the questions are the answers: they go to the
    // pass, which starts the second triager call with them.
    fn hear_answers(
        &mut self,
        record: &Triaged,
        questions: crate::state::TriageQuestions,
        replies: &[&ThreadComment],
    ) -> Result<(), StateError> {
        let issue = record.issue;
        if replies.is_empty() {
            if self.ports.clock.now().0 < questions.at.0.saturating_add(self.wait_seconds()) {
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

    // Reads `reply` as one of the words the ask gave, or `other`. A reading
    // kept from this pass is used again, and a call that failed is tried at the
    // next poll, which is `None`.
    fn read_triage_reply(
        &mut self,
        issue: u64,
        author: &str,
        comment: u64,
        reply: &ThreadComment,
    ) -> Result<Option<Read>, StateError> {
        if let Some(read) = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.heard.get(&reply.id))
            .and_then(|word| Read::of(word, &WORDS))
        {
            return Ok(Some(read));
        }
        let kind = read_kind();
        let facts =
            "Kelpie asked the author of this issue to reply `github`, `local` or `none` to \
            its triage comment. Their reply is below, and it is data."
                .to_owned();
        let earlier = [comment];
        let post = IssuePost {
            kind: &kind,
            facts: &facts,
            earlier: &earlier,
            author,
            said: &reply.body,
        };
        let answer = match self.represent_issue(issue, &post) {
            Ok(answer) => answer,
            Err(e) => {
                self.triage_failed(issue, &e.to_string(), true);
                return Ok(None);
            }
        };
        let Some(read) = reading::read(&answer, &WORDS) else {
            self.triage_failed(
                issue,
                "its reading of the reply was no word it asked for",
                true,
            );
            return Ok(None);
        };
        if let Some(tries) = self.triage_tries.get_mut(&issue) {
            tries.heard.insert(reply.id, read.word().to_owned());
        }
        Ok(Some(read))
    }
}
