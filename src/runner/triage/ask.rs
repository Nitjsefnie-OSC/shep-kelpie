//! The author's round: the ask, the questions, the answers
//!
//! An open question goes to the issue's author before it goes to the
//! maintainer. The triage comment asks them to settle it on GitHub, in their
//! own terminal, or not at all: reply `github` and kelpie posts its questions
//! there for them to answer, reply `local` and kelpie gives them one prompt to
//! run in a session of their own ([`super::local`]), reply `none` and the
//! maintainer triages the issue as it stands. Nothing is asked of an author
//! kelpie does not ask: only the maintainer's own issue, and any issue under
//! `pick = "all"` ([`Runner::askable`]).
//!
//! While the triage waits on a word from the author, no status label goes on:
//! [`Runner::finish_triage`] returns at a record whose `asked`, `questions` or
//! `local` are open, and the labels the pass owed wait in its record. The wait
//! holds no slot and no call. What the author writes is heard by
//! [`super::heard`], which reads only their own comments on that issue after
//! the ask, and read as one of the words it was given by
//! [`crate::runner::representative::reading`].
//!
//! The paths each field takes. `asked` is set by [`Runner::record_triage`]
//! when the triage comment ends with the ask, and again by
//! [`Runner::triage_ask_again`] with the plain ask that follows a reply kelpie could
//! not read as any of its words; it is cleared by [`Runner::set_questions`] when
//! the questions go up, by [`Runner::set_local`] when the prompt goes up, by
//! [`Runner::triage_none`] when the wait ends as `none`, by
//! [`Runner::triage_wait_stood`] when a triage label the maintainer
//! put on by hand or a closed issue ends it, and by [`Runner::record_triage`]
//! when a pass that asks nothing records its comment. `questions` is set by
//! [`Runner::set_questions`] and cleared by [`Runner::triage_none`],
//! [`Runner::triage_wait_stood`] and [`Runner::record_triage`], whose record
//! carries no questions once the answers round's comment is up. `local` is set
//! by [`Runner::set_local`] and cleared by the same three, and what its body
//! hash is measured against is in [`super::local`].

use super::super::Runner;
use super::super::contributions::post::{forge_is_busy, refused_text};
use super::super::representative::IssuePost;
use super::super::thread_replies::plain_login;
use super::TRIAGE_LABELS;
use super::research::Question;
use crate::board::TRIAGE;
use crate::ports::{ForgeError, OpenIssue, ThreadComment, Timestamp};
use crate::settings::TriagePick;
use crate::state::{StateError, TriageAsked, TriageQuestions, TriageStep, Triaged};

/// What marks kelpie's plain ask again, after a reply it could not read as
/// `github`, `local` or `none`
pub(in crate::runner) const ASK_MARKER: &str = "<!-- kelpie:triage ask -->";

/// What marks kelpie's questions comment, the one round it asks on GitHub
pub(in crate::runner) const QUESTIONS_MARKER: &str = "<!-- kelpie:triage questions -->";

/// What kind of post the representative is asked for beside the questions of
/// the one round kelpie asks on GitHub. The questions themselves are code's
/// and go under the lead-in.
const QUESTIONS_KIND: &str = "Write the short lead-in to kelpie's numbered questions for the \
    author of this issue. Greet them, say kelpie asks the open questions below and they can \
    answer them all in one comment, and say kelpie reads the answers and leaves what is still \
    open to the maintainer. Do not restate or renumber the questions: kelpie posts them under \
    your words. It is a comment, not a review: no verdict, and nothing promised.";

/// What kind of post the representative is asked for when kelpie asks an
/// author `github`, `local` or `none` once more, plainly
const ASK_AGAIN_KIND: &str = "Write the short comment that asks the author of this issue once \
    more, plainly, to reply `github`, `local` or `none` to kelpie's triage comment. Say this is \
    the last time kelpie asks, and that with no word either way the maintainer triages the issue \
    as it stands. Friendly and short. It is a comment, not a review: no verdict, and nothing \
    promised.";

/// What a triager call on an issue is for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::runner) enum Purpose {
    /// The first pass, the one that asks
    #[default]
    First,
    /// The pass that posts the questions, asked again where a restart lost them
    Questions,
    /// The pass that reads the author's answers
    Answers,
    /// The research that recovers what the local prompt names, which a restart
    /// lost before its comment went up
    LocalPrompt,
    /// The pass that reads the issue again after its author's own session
    /// rewrote its body
    LocalRead,
}

/// What writing the settled answers under the issue author's text came to
pub(in crate::runner) enum Write {
    /// It went on, and its step is saved
    Written,
    /// The forge refused it, so the issue ends `needs-triage` like any other,
    /// and the issue is told the write did not happen
    Refused,
    /// Nothing more this pass: the maintainer's own label stood, or the write
    /// failed and is tried at the next poll
    Held,
}

/// What the answers pass can put under the issue author's text: the whole body
/// to write, nothing to write, or a read of the issue that failed and is tried
/// again. The last is not the first: the round is still open and its comment is
/// not worded from a settled body kelpie does not have.
pub(in crate::runner) enum Settled {
    /// The author's text with what is settled under it
    Body(String),
    /// Nothing goes in the body: the pass settled no body, or what it settled
    /// names something of this machine
    Not,
    /// The issue's body could not be read, so the pass is tried again
    Held,
}

impl Runner {
    // Whether `author` is one kelpie asks to settle its open questions itself:
    // the maintainer on their own issue, and any author under `pick = "all"`.
    // Anyone else gets `needs-triage` with no ask.
    pub(in crate::runner) fn askable(&self, author: &str) -> bool {
        if self.settings.triage.pick == TriagePick::All {
            return true;
        }
        match self.maintainer() {
            Ok(login) => login.eq_ignore_ascii_case(author),
            Err(_) => false,
        }
    }

    // How long kelpie waits for the author's word, in seconds
    pub(super) fn wait_seconds(&self) -> u64 {
        u64::from(self.settings.triage.wait_days.get()).saturating_mul(86_400)
    }

    // The issues whose triage waits on the author, each with when the wait
    // began. Their time is what the thread read reaches back to, so a reply is
    // not missed, and what silence is measured from.
    pub(in crate::runner) fn triage_waits(&self) -> Vec<(u64, Timestamp)> {
        self.state.triage.as_ref().map_or_else(Vec::new, |triage| {
            triage
                .issues
                .iter()
                .filter_map(|record| {
                    let at = record
                        .questions
                        .map(|q| q.at)
                        .or_else(|| record.asked.map(|a| a.at));
                    at.map(|at| (record.issue, at))
                })
                .collect()
        })
    }

    // The issues whose triage waits on the author, in the order their records
    // hold them
    pub(super) fn triage_waiting(&self) -> Vec<Triaged> {
        self.state.triage.as_ref().map_or_else(Vec::new, |triage| {
            triage
                .issues
                .iter()
                .filter(|record| {
                    record.asked.is_some() || record.questions.is_some() || record.local.is_some()
                })
                .cloned()
                .collect()
        })
    }

    // Ends each wait the forge says is over: the issue is closed, or carries a
    // triage label the maintainer put on by hand. Neither is a word from the
    // author, so nothing is posted and no label kelpie owed goes on.
    pub(super) fn settle_triage_waits(&mut self, open: &[OpenIssue]) -> Result<(), StateError> {
        for record in self.triage_waiting() {
            let issue = record.issue;
            // A call on the issue is in flight. Ending its wait under the call
            // would let the call land on a record already saved finished and put
            // a comment and labels on an issue that is closed or already labelled
            // by hand. The next poll settles it once the call has landed.
            if self.flights.triaging() == Some(issue) {
                continue;
            }
            let Some(this) = open.iter().find(|i| i.number == issue) else {
                self.triage_wait_stood(
                    issue,
                    format!(
                        "triage #{issue}: it is closed, so the wait on its author ends with no \
                         post and no label"
                    ),
                )?;
                continue;
            };
            // The listing is where a restart gets the author's login back, which
            // is what tells their words from anyone else's.
            self.triage_tries.entry(issue).or_default().author = Some(this.author.clone());
            let seen = self.triage_seen(issue);
            let stood = (this.labels.iter()).find(|l| {
                TRIAGE_LABELS.contains(&l.as_str())
                    && !seen.iter().any(|s| s.as_str() == l.as_str())
            });
            if let Some(label) = stood {
                self.triage_wait_stood(
                    issue,
                    format!(
                        "triage #{issue}: `{label}` went on by hand, so the wait on its author \
                         ends with no post and no label"
                    ),
                )?;
            }
        }
        Ok(())
    }

    // Saves the record as finished with nothing owed: the wait ended because
    // something outside it did, so kelpie posts nothing and puts on no label.
    pub(in crate::runner) fn triage_wait_stood(
        &mut self,
        issue: u64,
        note: String,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let mut held = false;
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.asked = None;
            record.questions = None;
            record.local = None;
            record.labels.clear();
            record.ready_body = None;
            record.step = TriageStep::Done;
            held = true;
        }
        if held && let Some(triage) = next.triage.as_mut() {
            triage.prune();
        }
        self.save(next)?;
        self.triage_tries.remove(&issue);
        self.triage_due.retain(|d| d.issue != issue);
        self.triage_queue.retain(|&n| n != issue);
        self.notes.push(note);
        Ok(())
    }

    // Ends the wait as `none`: the ask and the questions come off the record,
    // and what the pass owed goes on, which is its labels and then
    // `needs-triage`. The triage comment stays as the maintainer's starting
    // point: nothing new is posted.
    pub(in crate::runner) fn triage_none(&mut self, issue: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.asked = None;
            record.questions = None;
            record.local = None;
        }
        self.save(next)?;
        if let Some(tries) = self.triage_tries.get_mut(&issue) {
            tries.answers = None;
            tries.questions_call = false;
            tries.local_call = false;
            tries.local_edit = false;
            tries.open.clear();
        }
        self.finish_triage(issue)
    }

    // The newest comment on `issue` that the App covering the repo wrote after
    // `after`, whose last line begins with `marker`. Kelpie's own markers in
    // anyone else's comment count for nothing. The whole comment comes back, so
    // a marker that carries a value can be read off it.
    pub(in crate::runner) fn app_comment_after(
        &self,
        issue: u64,
        marker: &str,
        after: Timestamp,
    ) -> Result<Option<ThreadComment>, ForgeError> {
        let comments = self.ports.forge.issue_comments(&self.remote, issue)?;
        let Some(app) = self.app_login().map(|app| plain_login(&app)) else {
            return Ok(None);
        };
        Ok(comments
            .iter()
            .filter(|c| {
                plain_login(&c.author) == app
                    && c.created > after
                    && c.body
                        .trim_end()
                        .lines()
                        .last()
                        .is_some_and(|line| line.starts_with(marker))
            })
            .max_by_key(|c| c.id)
            .cloned())
    }

    // Posts the one round of questions kelpie asks on GitHub: the numbered
    // questions with the answer each recommends, under a lead-in the
    // representative words. The questions come from the triager's reply, and
    // kelpie's code lists them.
    pub(in crate::runner) fn post_questions(
        &mut self,
        issue: u64,
        said: &str,
    ) -> Result<(), StateError> {
        let Some(record) = self.triage_record(issue).cloned() else {
            return Ok(());
        };
        let Some(asked) = record.asked else {
            return Ok(());
        };
        let questions = self
            .triage_tries
            .get(&issue)
            .map(|t| t.open.clone())
            .unwrap_or_default();
        if questions.is_empty() {
            self.notes.push(format!(
                "triage #{issue}: its author replied `github`, but the pass left no question to \
                 ask, so the maintainer triages it as it stands"
            ));
            return self.triage_none(issue);
        }
        let author = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.author.clone())
            .unwrap_or_default();
        // The forge may take a post whose answer is lost, so the marker is read
        // again before asking a second time.
        match self.app_comment_after(issue, QUESTIONS_MARKER, asked.at) {
            Ok(Some(prompt)) => return self.set_questions(issue, prompt.id),
            Ok(None) => {}
            Err(e) => {
                let why = format!("cannot read its comments: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        }
        let facts = questions_facts(questions.len());
        let kind = QUESTIONS_KIND;
        let earlier = [record.comment];
        let post = IssuePost {
            kind,
            facts: &facts,
            earlier: &earlier,
            author: &author,
            said,
        };
        let words = match self.represent_issue(issue, &post) {
            Ok(words) => words,
            Err(e) => {
                self.triage_failed(issue, &e.to_string(), true);
                return Ok(());
            }
        };
        let block = questions_block(&questions);
        let body = format!("{words}\n\n{block}\n\n{QUESTIONS_MARKER}");
        match self.ports.voice.post(&self.remote, issue, &body) {
            Ok(posted) => {
                if let Some(tries) = self.triage_tries.get_mut(&issue) {
                    tries.questions_call = false;
                }
                self.set_questions(issue, posted.id)
            }
            Err(e) => {
                let why = format!("cannot post the questions: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(())
            }
        }
    }

    // Saves the record as waiting on the answers, with the comment that holds
    // the questions and when it went up. The ask comes off: the questions
    // replace it, and their own `wait_days` is what silence is measured from.
    pub(in crate::runner) fn set_questions(
        &mut self,
        issue: u64,
        comment: u64,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.asked = None;
            record.questions = Some(TriageQuestions {
                comment,
                at: self.ports.clock.now(),
            });
        }
        self.save(next)
    }

    // Asks an author `github` or `none` once more, plainly, after a reply
    // kelpie could not read as either word. This is the last time it asks: a
    // second reply it cannot read counts as `none`.
    pub(in crate::runner) fn triage_ask_again(
        &mut self,
        issue: u64,
        said: &str,
    ) -> Result<(), StateError> {
        let Some(record) = self.triage_record(issue).cloned() else {
            return Ok(());
        };
        let Some(asked) = record.asked else {
            return Ok(());
        };
        let author = self
            .triage_tries
            .get(&issue)
            .and_then(|t| t.author.clone())
            .unwrap_or_default();
        match self.app_comment_after(issue, ASK_MARKER, asked.at) {
            Ok(Some(_)) => {}
            Ok(None) => {
                let facts = "Kelpie asked the author of this issue to reply `github`, `local` or \
                    `none` to its triage comment. Their reply was none of them. This is the last \
                    time kelpie asks: with no word either way the maintainer triages the issue as \
                    it stands."
                    .to_owned();
                let kind = ASK_AGAIN_KIND;
                let earlier = [record.comment];
                let post = IssuePost {
                    kind,
                    facts: &facts,
                    earlier: &earlier,
                    author: &author,
                    said,
                };
                let words = match self.represent_issue(issue, &post) {
                    Ok(words) => words,
                    Err(e) => {
                        self.triage_failed(issue, &e.to_string(), true);
                        return Ok(());
                    }
                };
                let body = format!("{words}\n\n{ASK_MARKER}");
                match self.ports.voice.post(&self.remote, issue, &body) {
                    Ok(_) => {}
                    Err(e) => {
                        let why = format!("cannot post the ask: {e}");
                        self.triage_failed(issue, &why, !forge_is_busy(&e));
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                let why = format!("cannot read its comments: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(());
            }
        }
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.asked = Some(TriageAsked {
                at: self.ports.clock.now(),
                asks: 2,
            });
        }
        self.save(next)
    }

    // Saves the record as commented with its labels still owed, which is what
    // the answers pass leaves behind whether its write went on or was refused:
    // the body is no longer owed, the type and priority the pass found are.
    // `triage_stepped` would clear them with the body, so this keeps them.
    pub(super) fn triage_answers_commented(&mut self, issue: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(record) = next
            .triage
            .as_mut()
            .and_then(|t| t.issues.iter_mut().find(|t| t.issue == issue))
        {
            record.step = TriageStep::Commented;
            record.ready_body = None;
        }
        self.save(next)
    }

    // Writes the settled answers under `issue`'s author's text and saves its
    // step, unless a triage label the maintainer put on by hand stands, which
    // leaves the record with no further label and no write.
    pub(super) fn answers_write(&mut self, issue: u64, body: &str) -> Result<Write, StateError> {
        match self.triage_labelled_by_hand(issue) {
            Ok(Some(label)) => {
                self.notes.push(format!(
                    "triage #{issue}: `{label}` went on by hand, so that stands: kelpie writes \
                     no answers into its body and labels it no further"
                ));
                self.triage_wait_stood(
                    issue,
                    format!("triage #{issue}: `{label}` stands, and its answers are not written"),
                )?;
                return Ok(Write::Held);
            }
            Ok(None) => {}
            Err(e) => {
                let why = format!("cannot read its labels: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Ok(Write::Held);
            }
        }
        match self.ports.voice.edit_issue_body(&self.remote, issue, body) {
            Ok(()) => {
                self.triage_body_written(issue, body)?;
                self.triage_answers_commented(issue)?;
                Ok(Write::Written)
            }
            Err(e) if refused_text(&e) => {
                self.notes.push(format!(
                    "triage #{issue}: its answers were refused: {e}; they are not written, and \
                     the issue ends `{TRIAGE}"
                ));
                // The step moves to `commented` here rather than through
                // `triage_ready_dropped`, which would leave it at `answers` for
                // `finish_triage` to step with `triage_stepped`, and that clears
                // the labels the pass found with the body.
                self.triage_answers_commented(issue)?;
                // The comment said the answers would go under the author's text.
                // Say on the issue that they did not, rather than leave that
                // promise there unkept.
                self.notice_on(
                    issue,
                    "Kelpie did not write the answers into this issue's body: the text it was \
                     given was refused. The body stays as its author wrote it, and the maintainer \
                     triages it next.",
                );
                Ok(Write::Refused)
            }
            Err(e) => {
                let why = format!("cannot write its body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(Write::Held)
            }
        }
    }

    // The issue's body with the settled answers under its author's own text.
    // [`Settled::Held`] is the forge failing to read it, which is tried again:
    // the round stays open and no comment is worded without the body. The
    // author's text is never written over.
    pub(in crate::runner) fn settled_answers(&mut self, issue: u64, settled: &str) -> Settled {
        let body = match self.ports.forge.issue(&self.remote, issue) {
            Ok(found) => found.body,
            Err(e) => {
                let why = format!("cannot read its body: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return Settled::Held;
            }
        };
        let whole = format!("{}\n\n{}", body.trim(), settled.trim());
        if let Some(leak) = self.local.find(&whole, crate::local_paths::Surface::Prose) {
            self.notes.push(format!(
                "triage #{issue}: the answers it would write name {leak}, so they are not \
                 written and the issue ends `{TRIAGE}"
            ));
            return Settled::Not;
        }
        Settled::Body(whole)
    }

    // The questions kelpie posted for `issue` and its author's answers, which
    // the second pass reads fenced with the issue. `None` when neither can be
    // had, which is no call this poll.
    pub(super) fn answers_round(&mut self, issue: u64) -> Option<(String, String)> {
        let asked = self.triage_record(issue)?.questions?;
        let answers = self.triage_tries.get(&issue)?.answers.clone()?;
        let round = match self.ports.forge.issue_comments(&self.remote, issue) {
            Ok(comments) => {
                (comments.iter().find(|c| c.id == asked.comment)).map(|c| c.body.clone())
            }
            Err(e) => {
                let why = format!("cannot read its questions: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return None;
            }
        };
        // A questions comment that cannot be read is stood for by the questions
        // the pass still holds. With neither, there is nothing to put beside the
        // issue, and no call this poll.
        let asked = match round.or_else(|| {
            let open = self
                .triage_tries
                .get(&issue)
                .map(|t| t.open.clone())
                .unwrap_or_default();
            (!open.is_empty()).then(|| questions_block(&open))
        }) {
            Some(asked) => asked,
            None => {
                self.notes.push(format!(
                    "triage #{issue}: its questions comment is gone and its questions were lost, \
                     so its answers are not read and the second pass waits for the next poll"
                ));
                return None;
            }
        };
        Some((asked, answers))
    }
}

// The questions as the grilling skill shapes a round, without its bold: each
// numbered, each with the answer the research recommends, each apart from the
// next. Code writes them; the representative words only the lead-in.
fn questions_block(questions: &[Question]) -> String {
    let round: Vec<String> = questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let question = format!("❓ Q{} - {}", i + 1, q.question.trim());
            match q.recommended.trim() {
                "" => question,
                recommended => format!("{question}\n\n➡️ {recommended}"),
            }
        })
        .collect();
    round.join("\n\n---\n\n")
}

// What the representative words the lead-in from: that the author chose GitHub
// for this, and how many questions kelpie posts under the lead-in. The
// questions themselves are not facts to restate.
fn questions_facts(most: usize) -> String {
    let mut facts = String::from(
        "Kelpie asked the author of this issue to settle its open questions, and they replied \
         `github`.\n",
    );
    let many = match most == 1 {
        true => "",
        false => "s",
    };
    facts.push_str(&format!(
        "\nKelpie posts {most} numbered question{many} below your words, each with the answer \
         its research recommends.\n"
    ));
    facts.push_str(
        "\nThe author answers them in one comment. Kelpie reads the answers, and what is still \
         open after that one round goes to the maintainer.\n",
    );
    facts
}
