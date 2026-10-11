//! A ruling's comment once someone else has settled it
//!
//! A ruling can end any way but the maintainer's reply in its thread: the
//! project manager answers it, `shep kelpie rule` does at the terminal, a reply
//! to the alert does, or kelpie withdraws it. Its comment, posted through the
//! App, would stay up as an open question, so it is edited to say it is
//! settled and by whom, and the lines that invite an answer go. The same
//! words tell a later reply in the thread what became of the ruling.

use super::super::voice::settled_comment;
use super::super::words::NO_NOTE;
use super::super::{Answer, RuleError};
use super::{LATE, Runner};
use crate::ports::Timestamp;
use crate::state::Ruling;

/// What a settled ruling's comment says after its first line
const NOTHING_WAITS: &str = "Nothing waits on the maintainer.";

/// Why an answer about a head that is gone withdraws its ruling
const MOVED: &str = "the branch moved while it waited, so kelpie looks at what stands now";

/// Who settled a ruling
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runner) enum Settler {
    /// `shep kelpie rule`, at the terminal
    Terminal,
    /// A reply to the alert
    Alert,
    /// The maintainer, in the ruling's thread: a reply, a reaction or a tick
    Thread,
    /// The project manager, with its answer in words and its one-line why
    Pm { answer: String, why: String },
}

/// What came of a ruling that is gone from the state
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runner) struct Outcome {
    /// The first line of its edited comment, none where the comment is left
    line: Option<String>,
    /// What a later reply in its thread is told, after "Ruling 1 "
    told: String,
}

impl Outcome {
    /// `settler`'s answer
    pub(in crate::runner) fn answered(settler: &Settler, answer: &Answer) -> Self {
        let words = bare(&words(answer));
        let settled = |how: String, told: String| Self {
            line: Some(format!("**Settled.** {how}. {NOTHING_WAITS}")),
            told,
        };
        match settler {
            Settler::Terminal => settled(
                format!("Answered at the terminal with `shep kelpie rule`: {words}"),
                format!("is already answered at the terminal with `shep kelpie rule`: {words}"),
            ),
            Settler::Alert => settled(
                format!("Answered by a reply to the alert: {words}"),
                format!("is already answered by a reply to the alert: {words}"),
            ),
            Settler::Thread => Self {
                line: None,
                told: format!("is already answered in its own thread: {words}"),
            },
            Settler::Pm { answer, why } => {
                let (answer, why) = (bare(answer), bare(why));
                let because = match why.is_empty() {
                    true => String::new(),
                    false => format!(" (why: {why})"),
                };
                settled(
                    format!("The project manager answered: {answer}{because}"),
                    format!("is already answered by the project manager: {answer}"),
                )
            }
        }
    }

    /// A ruling kelpie took back, for `why`
    pub(in crate::runner) fn withdrawn(why: &str) -> Self {
        let why = bare(why);
        Self {
            line: Some(format!("**Withdrawn:** {why}. {NOTHING_WAITS}")),
            told: format!("is withdrawn: {why}"),
        }
    }

    /// This outcome, with its comment left as it is where `left` is set
    fn in_thread(self, left: bool) -> Self {
        match left {
            true => Self { line: None, ..self },
            false => self,
        }
    }
}

/// A ruling that ended while its comment was up, kept until its end is said
#[derive(Debug, Clone)]
pub(super) struct Ended {
    pub(super) ruling: Ruling,
    /// The id of its comment on the thread
    pub(super) comment: u64,
}

impl Runner {
    /// Answers ruling `id` as `by` gave it, and where the ruling's comment is
    /// up, says in it that someone other than the maintainer, in its thread,
    /// settled it
    ///
    /// # Errors
    ///
    /// As [`Runner::rule`].
    pub(in crate::runner) fn rule_by(
        &mut self,
        id: u64,
        answer: Answer,
        by: Settler,
    ) -> Result<(), RuleError> {
        let outcome = Outcome::answered(&by, &answer);
        let outcome = match self.apply(id, answer)? {
            true => outcome,
            // The answer was about a head that is gone, so none was taken. An
            // answer in the ruling's own thread leaves its comment as it is.
            false => Outcome::withdrawn(MOVED).in_thread(by == Settler::Thread),
        };
        self.say_settled(id, outcome);
        Ok(())
    }

    /// Says what became of ruling `id`, which the last save took out of the
    /// state: its comment is edited, where it was posted and `outcome` calls
    /// for it, and a reply in its thread is told the same
    pub(in crate::runner) fn say_settled(&mut self, id: u64, outcome: Outcome) {
        self.threads.told.insert(id, outcome.told);
        let Some(ended) = self.threads.ended.remove(&id) else {
            return;
        };
        let Some(line) = outcome.line else {
            return;
        };
        if !self.app_covers() {
            return;
        }
        let body = settled_comment(&ended.ruling, &line);
        if let Err(e) = (self.ports.voice).edit_comment(&self.remote, ended.comment, &body) {
            self.notes.push(format!(
                "cannot edit the comment of ruling {id} to say it is settled, and will try \
                 again for a while: {e}"
            ));
            let until = Timestamp(self.ports.clock.now().0.saturating_add(LATE));
            (self.threads.unedited).insert(id, (ended.comment, body, until));
        }
    }

    /// Tries again each edit of a settled ruling's comment that failed, and
    /// lets go of the ones that went through or that have waited too long
    pub(super) fn retry_edits(&mut self, now: Timestamp) {
        let wanted = std::mem::take(&mut self.threads.unedited);
        for (id, (comment, body, until)) in wanted {
            // The failure was told when it first happened, not at each poll.
            let sent = (self.ports.voice).edit_comment(&self.remote, comment, &body);
            if sent.is_err() && now < until {
                self.threads.unedited.insert(id, (comment, body, until));
            }
        }
    }
}

// An answer in a few words.
fn words(answer: &Answer) -> String {
    let with = |lead: &str, note: &str| match note.trim() {
        "" => lead.to_owned(),
        note if note == NO_NOTE => lead.to_owned(),
        note => format!("{lead}, {note}"),
    };
    match answer {
        Answer::Yes => "yes".to_owned(),
        Answer::No(note) => with("no", note),
        Answer::Rework(note) => with("rework", note),
        Answer::Text(text) => text.clone(),
    }
}

// `text` on one line, with no full stop to end it, to sit inside a sentence.
fn bare(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    line.trim_end_matches('.').to_owned()
}
