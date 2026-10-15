//! Answering what a contributor asks
//!
//! Once a poll, each comment or review reply the contributor wrote that is
//! not yet answered goes to the representative in one call, given their
//! words (fenced as data), the findings the reviewers recorded on the head,
//! the diff since the base, and the maintainer's recent comments for
//! context. It replies with JSON: an `answer`, which is posted as a comment
//! through the App, or a `decision`, a question for the maintainer, which
//! raises a ruling and posts nothing yet. The maintainer's answer to that
//! ruling goes back through the representative, which words it for the
//! contributor, and that is posted.
//!
//! The representative never promises a merge: only a ruling decides one.
//! A bot's comment is never heard, and the maintainer's is context and not
//! answered. Each heard item is answered at most once, and the record keeps
//! its id. Calls stop at `budget.per_item_usd` as every other call on a
//! contribution does, and while a ruling waits on the pull request. A call
//! that fails, or a post the forge refuses, is told once in the log and tried
//! again at the next poll, up to [`POST_TRIES`] times, and never fails the
//! poll.

use serde::Deserialize;

use super::super::Runner;
use super::super::representative::Post;
use super::post::{POST_TRIES, Wording, forge_is_busy, refused_text, short};
use super::release::awaits_release;
use super::ruling::{flagged_lines, raise_question};
use crate::ports::{Cost, Finding};
use crate::state::{Contribution, Deciding, Said, StateError};
use crate::worktree;

/// The longest the contributor's words, or one of the maintainer's, are
/// given to the call, in characters
const LONGEST_SAID: usize = 8_000;

/// The longest the diff is given to the call, in bytes
const LONGEST_DIFF: usize = 60_000;

/// The longest the question to the maintainer is, in characters: the
/// representative wrote it from the contributor's text, and it goes to the
/// maintainer's webhook
const LONGEST_ASK: usize = 600;

/// How many of the maintainer's latest comments are given as context
const CONTEXT: usize = 5;

/// What kind of post the representative is asked for
const ANSWER: &str = "Answer the contributor's latest comment on their pull request. Reply with \
    only JSON. If the facts and the diff answer it, reply {\"kind\": \"answer\", \"text\": \
    \"...\"}, where \"text\" is the comment to post. If they ask for a decision, such as whether \
    something will be merged, accepted, allowed, waived or scheduled, or if the facts and the \
    diff do not answer it, reply {\"kind\": \"decision\", \"question\": \"...\"}, where \
    \"question\" is one clear question to the maintainer, and write nothing for the \
    contributor. Never promise or hint at a merge: only the maintainer's ruling decides one. \
    Say only what the facts and the diff show. The maintainer's comments are context, and they \
    need no answer.";

const RELAY: &str = "The contributor asked for a decision and the maintainer has answered. Write \
    the comment that gives the contributor the maintainer's answer, in your own voice. Reply with \
    only the comment. Say what the maintainer said and nothing more. Never promise or hint at a \
    merge: only the maintainer's ruling decides one.";

// What the representative replied
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Reply {
    // A comment to post
    Answer { text: String },
    // A question for the maintainer
    Decision { question: String },
}

// What is due on one contribution
enum Due {
    // A heard comment or reply to answer
    Answer(Said),
    // The maintainer's answer to a decision, to word and post
    Relay(Deciding),
}

impl Runner {
    // Answers what each contribution's contributor said that is not yet
    // answered, and words and posts the maintainer's answers to the decisions
    // it asked for.
    pub(super) fn follow_answers(&mut self) -> Result<(), StateError> {
        if self.settings.contributions.is_off() || !self.app_covers() {
            return Ok(());
        }
        let numbers: Vec<u64> = (self.state.contributions.iter())
            .map(|c| c.pull_request)
            .collect();
        for number in numbers {
            while let Some(due) = self.answer_due(number) {
                // A ruling on the pull request holds its calls, whatever
                // it is about, and a spend past the cap raises one.
                if self.ruling_waits(number) || self.over_budget(number)? {
                    break;
                }
                let done = match due {
                    Due::Answer(said) => self.answer_one(number, said)?,
                    Due::Relay(deciding) => self.relay(number, deciding)?,
                };
                if !done {
                    break;
                }
            }
        }
        Ok(())
    }

    // The next thing to do for the contribution: the maintainer's answer
    // first, then the oldest comment or reply not yet answered that is not
    // given up. Nothing while a decision waits on the maintainer. An answer
    // whose relay is given up no longer holds the comments behind it.
    fn answer_due(&self, number: u64) -> Option<Due> {
        let held = (self.state.contributions.iter()).find(|c| c.pull_request == number)?;
        if let Some(deciding) = &held.deciding {
            let given_up = self.answer_given_up(&relay_key(number, &deciding.said));
            match (&deciding.answer, given_up) {
                (None, _) => return None,
                (Some(_), false) => return Some(Due::Relay(deciding.clone())),
                (Some(_), true) => {}
            }
        }
        (held.unanswered().into_iter())
            .filter(|said| !awaits_release(held, said))
            .find(|said| !self.answer_given_up(&answer_key(number, said)))
            .map(Due::Answer)
    }

    // Whether a decision of the contributor's is open on the pull request
    fn decision_open(&self, number: u64) -> bool {
        (self.state.contributions.iter()).any(|c| c.pull_request == number && c.deciding.is_some())
    }

    // Whether a ruling on the pull request waits for the maintainer
    pub(super) fn ruling_waits(&self, number: u64) -> bool {
        (self.state.rulings.iter()).any(|r| r.id == number)
    }

    // Asks the representative about `said`. Returns whether it was dealt
    // with, so the next can be.
    fn answer_one(&mut self, number: u64, said: Said) -> Result<bool, StateError> {
        let key = answer_key(number, &said);
        if let Some(id) = self.sent_before(&key) {
            self.record_answered(number, &said, id, false)?;
            return Ok(true);
        }
        let Some(held) = (self.state.contributions.iter())
            .find(|c| c.pull_request == number)
            .cloned()
        else {
            return Ok(false);
        };
        let text = match self.kept_answer(&key) {
            Some(text) => text,
            None => {
                let Some(replied) = self.ask_about(&held, &said, &key)? else {
                    return Ok(false);
                };
                match replied {
                    Reply::Decision { .. } if self.decision_open(number) => {
                        // A relay given up keeps its decision, and a second
                        // would overwrite the maintainer's saved answer.
                        let why = "an earlier decision is still open";
                        self.answer_failed(&key, number, "answer", &why, true);
                        return Ok(false);
                    }
                    Reply::Decision { question } => {
                        self.raise_decision(number, said, &question)?;
                        self.answer_ends(&key);
                        return Ok(false);
                    }
                    Reply::Answer { text } => {
                        self.keep_answer(&key, &text);
                        text
                    }
                }
            }
        };
        match self.send_answer(number, &key, &text, "answer") {
            Some(id) => {
                self.record_answered(number, &said, id, false)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // Words the maintainer's answer for the contributor and posts it.
    fn relay(&mut self, number: u64, deciding: Deciding) -> Result<bool, StateError> {
        let key = relay_key(number, &deciding.said);
        if let Some(id) = self.sent_before(&key) {
            self.record_answered(number, &deciding.said, id, true)?;
            return Ok(true);
        }
        let text = match self.kept_answer(&key) {
            Some(text) => text,
            None => {
                let Some(held) = (self.state.contributions.iter())
                    .find(|c| c.pull_request == number)
                    .cloned()
                else {
                    return Ok(false);
                };
                let Some(text) = self.word_relay(&held, &deciding, &key)? else {
                    return Ok(false);
                };
                self.keep_answer(&key, &text);
                text
            }
        };
        match self.send_answer(number, &key, &text, "reply") {
            Some(id) => {
                self.record_answered(number, &deciding.said, id, true)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    // Calls the representative on the comment `said` and reads its reply, or
    // `None` when the words could not be had or the call failed, told in the
    // log and tried at the next poll. What the call cost is added to the
    // contribution's spend whatever it answered.
    fn ask_about(
        &mut self,
        held: &Contribution,
        said: &Said,
        key: &str,
    ) -> Result<Option<Reply>, StateError> {
        let number = held.pull_request;
        let wrote = match self.said_text(number, said) {
            Ok(wrote) => wrote,
            Err(why) => {
                self.answer_failed(key, number, "answer", &why, true);
                return Ok(None);
            }
        };
        let facts = self.answer_facts(held);
        let post = Post {
            pull_request: number,
            kind: ANSWER,
            facts: &facts,
            earlier: &[],
            contributor: &held.author,
            said: &wrote,
        };
        let (words, cost) = match self.represent_priced(&post) {
            Ok(done) => done,
            Err(e) => {
                self.answer_failed(key, number, "answer", &e, true);
                return Ok(None);
            }
        };
        self.add_spend(number, cost)?;
        match read_reply(&words) {
            Some(reply) => Ok(Some(reply)),
            None => {
                let why = "its reply is not the JSON asked for";
                self.answer_failed(key, number, "answer", &why, true);
                Ok(None)
            }
        }
    }

    // The maintainer's answer to the decision as the representative words it
    // for the contributor, or `None` when it could not be had.
    fn word_relay(
        &mut self,
        held: &Contribution,
        deciding: &Deciding,
        key: &str,
    ) -> Result<Option<String>, StateError> {
        let number = held.pull_request;
        let wrote = match self.said_text(number, &deciding.said) {
            Ok(wrote) => wrote,
            Err(why) => {
                self.answer_failed(key, number, "reply", &why, true);
                return Ok(None);
            }
        };
        let facts = format!(
            "The contributor asked for a decision, and you put this question to the maintainer: \
             {}\n\nThe maintainer answered: {}\n",
            deciding.asked,
            deciding.answer.as_deref().unwrap_or_default()
        );
        let post = Post {
            pull_request: number,
            kind: RELAY,
            facts: &facts,
            earlier: &[],
            contributor: &held.author,
            said: &wrote,
        };
        match self.represent_priced(&post) {
            Ok((words, cost)) => {
                self.add_spend(number, cost)?;
                Ok(Some(words))
            }
            Err(e) => {
                self.answer_failed(key, number, "reply", &e, true);
                Ok(None)
            }
        }
    }

    // The facts the call may state: the findings recorded on the head, the
    // diff since the base and the maintainer's recent comments, the last two
    // as outside text.
    fn answer_facts(&self, held: &Contribution) -> String {
        let mut facts = format!(
            "Pull request #{} by @{}, at {}.\n",
            held.pull_request,
            held.author,
            short(&held.head)
        );
        let (findings, nits) = match held.pass.as_ref().filter(|p| p.head == held.head) {
            Some(pass) => (pass.findings.as_slice(), pass.nits.as_slice()),
            None => (&[][..], &[][..]),
        };
        facts.push_str(&listed(
            "What the reviewers found on this head",
            "No reviewer has recorded a finding on this head.",
            findings,
        ));
        if !nits.is_empty() {
            facts.push_str(&listed("Nits a review bot left", "", nits));
        }
        let diff = self.diff_text(held);
        facts.push_str(&format!(
            "\n{}\n",
            crate::runner::untrusted::fenced("The diff since main", &diff)
        ));
        let context = self.maintainer_text(held);
        if !context.is_empty() {
            facts.push_str(&format!(
                "\n{}\n",
                crate::runner::untrusted::fenced(
                    "The maintainer's latest comments on the pull request, for context only",
                    &context
                )
            ));
        }
        facts
    }

    // The change the contribution's current head makes from main, read from
    // the head itself, fetched first, and not from a checkout that may still
    // hold an earlier head. Cut at [`LONGEST_DIFF`] bytes, or what is wrong
    // with getting it.
    fn diff_text(&self, held: &Contribution) -> String {
        let read = worktree::contribution_change(
            &self.settings.git.checkout,
            held.pull_request,
            &held.head,
            None,
            LONGEST_DIFF,
        );
        match read {
            Ok(change) if change.cut => format!("{}\n[the diff is cut here]", change.diff),
            Ok(change) => change.diff,
            Err(why) => format!("[the diff cannot be read: {why}]"),
        }
    }

    // The maintainer's latest comments, as far as they can be read
    fn maintainer_text(&self, held: &Contribution) -> String {
        let start = held.context.len().saturating_sub(CONTEXT);
        let texts: Vec<String> = (held.context[start..].iter())
            .filter_map(|said| self.said_text(held.pull_request, said).ok())
            .collect();
        texts.join("\n\n---\n\n")
    }

    // What the comment or reply `said` says now, read again from the forge
    pub(super) fn said_text(&self, number: u64, said: &Said) -> Result<String, String> {
        let forge = &self.ports.forge;
        let text = match said {
            Said::Comment { id, .. } => forge
                .comment_marks(&self.remote, *id)
                .map(|marks| marks.body)
                .map_err(|e| format!("cannot read comment {id}: {e}"))?,
            Said::Reply { id, at } => {
                let replies = forge
                    .thread_replies_since(&self.remote, number, *at)
                    .map_err(|e| format!("cannot read the review threads: {e}"))?;
                (replies.into_iter().find(|r| r.id == *id))
                    .map(|r| r.body)
                    .ok_or_else(|| format!("review reply {id} is not there"))?
            }
            Said::Push { .. } => return Err("a push says nothing".to_owned()),
        };
        Ok(cut(&text, LONGEST_SAID, "\n[cut here]"))
    }

    // Raises the ruling for a decision the contributor asked for, and keeps
    // what the maintainer's answer needs. The comment is answered by then, as
    // far as the record goes.
    fn raise_decision(
        &mut self,
        number: u64,
        said: Said,
        question: &str,
    ) -> Result<(), StateError> {
        // Another ruling raised since the call keeps the pull request's id:
        // the comment stays unanswered and is asked again once it is gone.
        if self.ruling_waits(number) || self.decision_open(number) {
            return Ok(());
        }
        let asked = cut(&one_line(question), LONGEST_ASK, "...");
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) else {
            return Ok(());
        };
        held.answer(said.clone());
        held.deciding = Some(Deciding {
            said,
            asked: asked.clone(),
            answer: None,
        });
        raise_question(&mut next, number, asked, now);
        self.save(next)
    }

    // Adds `cost` to the contribution's spend
    pub(super) fn add_spend(&mut self, number: u64, cost: Cost) -> Result<(), StateError> {
        if cost.is_free() {
            return Ok(());
        }
        let mut next = self.state.clone();
        if let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) {
            held.spend = Cost(held.spend.0.saturating_add(cost.0));
            self.save(next)?;
        }
        Ok(())
    }

    // Posts `text` on the pull request and returns the id the forge gave, or
    // `None` and the failure told in the log.
    pub(super) fn send_answer(
        &self,
        number: u64,
        key: &str,
        text: &str,
        what: &str,
    ) -> Option<u64> {
        match (self.ports.voice).post(&self.remote, number, text) {
            Ok(posted) => Some(posted.id),
            Err(e) => {
                self.answer_failed(key, number, what, &e, !forge_is_busy(&e));
                if refused_text(&e) {
                    self.drop_answer(key);
                }
                None
            }
        }
    }

    // Records that `said` was dealt with by the comment `id`, after the
    // forge took it, and clears the decision it answered for a `relay`. A
    // save that fails leaves the id to be recorded at the next poll.
    fn record_answered(
        &mut self,
        number: u64,
        said: &Said,
        id: u64,
        relay: bool,
    ) -> Result<(), StateError> {
        let key = match relay {
            true => relay_key(number, said),
            false => answer_key(number, said),
        };
        let mut next = self.state.clone();
        if let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) {
            held.answer(said.clone());
            if relay {
                held.deciding = None;
            }
        }
        match self.save(next) {
            Ok(()) => {
                self.answer_ends(&key);
                Ok(())
            }
            Err(e) => {
                let mut tries = self.post_tries.borrow_mut();
                tries.entry(key).or_default().sent = Some(id);
                Err(e)
            }
        }
    }

    // The id of a post the forge took whose record was not saved
    pub(super) fn sent_before(&self, key: &str) -> Option<u64> {
        self.post_tries.borrow().get(key)?.sent
    }

    // The text an earlier try worded, so a post that fails is sent again
    // without asking the representative again
    pub(super) fn kept_answer(&self, key: &str) -> Option<String> {
        let tries = self.post_tries.borrow();
        Some(tries.get(key)?.wording.as_ref()?.body.clone())
    }

    pub(super) fn keep_answer(&self, key: &str, text: &str) {
        let mut tries = self.post_tries.borrow_mut();
        let wording = Wording::comment(text.to_owned());
        tries.entry(key.to_owned()).or_default().wording = Some(wording);
    }

    // The text is worded again at the next try, since it may have been what
    // the guard refused
    pub(super) fn drop_answer(&self, key: &str) {
        if let Some(attempt) = self.post_tries.borrow_mut().get_mut(key) {
            attempt.wording = None;
        }
    }

    // Whether the answer failed [`POST_TRIES`] times, which ends the trying
    // until kelpie restarts
    pub(super) fn answer_given_up(&self, key: &str) -> bool {
        let tries = self.post_tries.borrow();
        tries.get(key).is_some_and(|a| a.failures >= POST_TRIES)
    }

    // A try that worked leaves nothing to keep
    pub(super) fn answer_ends(&self, key: &str) {
        self.post_tries.borrow_mut().remove(key);
    }

    // Says in the log that `what` could not be done for the pull request,
    // once, and again when it is given up. A failure the forge being busy
    // explains is not counted, since asking again later may work.
    pub(super) fn answer_failed(
        &self,
        key: &str,
        number: u64,
        what: &str,
        why: &dyn std::fmt::Display,
        counts: bool,
    ) {
        let mut tries = self.post_tries.borrow_mut();
        let attempt = tries.entry(key.to_owned()).or_default();
        if counts {
            attempt.failures += 1;
        }
        if attempt.failures >= POST_TRIES {
            self.notes.push(format!(
                "contribution #{number}: cannot {what} the contributor: {why}; it failed \
                 {POST_TRIES} times, so it is not tried again until kelpie restarts"
            ));
        } else if !attempt.told {
            attempt.told = true;
            self.notes.push(format!(
                "contribution #{number}: cannot {what} the contributor: {why}; it is tried at \
                 the next poll"
            ));
        }
    }
}

// A comment is told apart by its kind and id, as the record does
pub(super) fn said_key(said: &Said) -> String {
    match said {
        Said::Comment { id, .. } => format!("comment-{id}"),
        Said::Reply { id, .. } => format!("reply-{id}"),
        Said::Push { head, .. } => format!("push-{head}"),
    }
}

fn answer_key(number: u64, said: &Said) -> String {
    format!("answer:{number}:{}", said_key(said))
}

fn relay_key(number: u64, said: &Said) -> String {
    format!("relay:{number}:{}", said_key(said))
}

// The representative's reply as an answer or a decision, or `None` when it
// is not the JSON asked for or says nothing
fn read_reply(said: &str) -> Option<Reply> {
    let (start, end) = (said.find('{')?, said.rfind('}')?);
    let reply: Reply = serde_json::from_str(said.get(start..=end)?).ok()?;
    let text = match &reply {
        Reply::Answer { text } => text,
        Reply::Decision { question } => question,
    };
    (!text.trim().is_empty()).then_some(reply)
}

// `title`, then each finding, or `none` when there is no finding
fn listed(title: &str, none: &str, findings: &[Finding]) -> String {
    match findings.is_empty() {
        true => format!("{none}\n"),
        false => format!("{title}:\n{}\n", flagged_lines(findings)),
    }
}

// `text` in one line
pub(super) fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// `text` cut at `longest` characters, with `mark` after the cut
pub(super) fn cut(text: &str, longest: usize, mark: &str) -> String {
    match text.char_indices().nth(longest) {
        Some((end, _)) => format!("{}{mark}", &text[..end]),
        None => text.to_owned(),
    }
}
