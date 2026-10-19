//! Posting a local reviewer's pass to the contributor
//!
//! Once a poll, a pass over the current head that a reviewer other than a
//! review bot answered is posted as the App, in the representative's words:
//! findings as one review that only comments, a comment on each finding the
//! diff shows a line for and the rest in the review's body, and a pass that
//! found nothing as one short note. A bot's threads are its own post, so a
//! pass a bot answered posts nothing. Each is posted once per head: the id
//! the forge gave is kept in the pass, and a push clears the pass with it.
//!
//! The representative words the review in one call, as JSON of the body and
//! a list of comments in finding order. An answer that is not that, or has a
//! different count of comments, falls back to the findings' own words. A
//! call that fails, or a post the forge refuses, is told once in the log and
//! tried again at the next poll, and never fails the poll.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::super::Runner;
use super::super::representative::{Post, neutered};
use super::super::review::calls::diff_against;
use super::pass::FOLDER;
use crate::ports::{Finding, ForgeError, LineComment, Severity};
use crate::state::StateError;

/// What kind of post the representative is asked for
const REVIEW: &str = "Write a pull request review of the contributor's latest push from the \
    local reviewer's findings. It is one review: an opening text, and one comment for each line \
    finding. Reply with only JSON of the form {\"body\": \"...\", \"comments\": [\"...\"]}, where \
    \"comments\" holds exactly one string for each line finding, in the order given, and the \
    kelpie adds each finding listed for the body under your body in the finding's own words, so do \
    not repeat those.";

const NOTE: &str = "Tell the contributor that the local reviewer read their latest push and \
    found nothing, and that the pull request is with the maintainer now. Short.";

// What is left to post for one contribution's head
enum Due {
    // The findings, as a review
    Review(Vec<Finding>),
    // That there are none, as a note
    Note,
}

struct Work {
    number: u64,
    head: String,
    author: String,
    due: Due,
}

// What a post has to keep once the forge has it
#[derive(Debug, Clone, Copy)]
pub(in crate::runner) enum Posted {
    Review(u64),
    Note(u64),
}

// The post as worded, kept in memory until it is posted
#[derive(Debug, Clone)]
pub(in crate::runner) struct Wording {
    pub(super) body: String,
    comments: Vec<LineComment>,
}

impl Wording {
    // A post that is a comment with a body and no line comments
    pub(super) fn comment(body: String) -> Self {
        Self {
            body,
            comments: Vec::new(),
        }
    }
}

/// What the runner keeps of one post that has not gone through: how many
/// times it failed, whether the log has said so, and its wording
#[derive(Debug, Default)]
pub(in crate::runner) struct Attempt {
    pub(super) failures: u32,
    pub(super) told: bool,
    pub(super) wording: Option<Wording>,
    /// The id of the comment the forge took whose record was not saved, so
    /// it is recorded at the next poll and never posted again
    pub(super) sent: Option<u64>,
}

/// How many times a post may fail for a head before kelpie stops trying it
pub(super) const POST_TRIES: u32 = 5;

// The review as the representative worded it: the opening text, and a comment
// for each finding that has a line
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Words {
    body: String,
    comments: Vec<String>,
}

impl Runner {
    // Posts what each contribution's pass over its current head has to say
    // and has not yet said.
    pub(super) fn follow_posts(&mut self) -> Result<(), StateError> {
        if self.settings.contributions.is_off() || !self.app_covers() {
            return Ok(());
        }
        // What the forge took and the state did not keep is kept first, so
        // it is not posted twice.
        let unsaved = std::mem::take(&mut *self.post_unsaved.borrow_mut());
        for (number, head, posted) in unsaved {
            self.keep_posted(number, &head, posted)?;
        }
        for work in self.posts_due() {
            let posted = match &work.due {
                Due::Review(findings) => self.post_review(&work, findings),
                Due::Note => self.post_note(&work),
            };
            if let Some(posted) = posted {
                self.keep_posted(work.number, &work.head, posted)?;
            }
        }
        Ok(())
    }

    // The posts the state owes: a pass a listed reviewer that is no review
    // bot answered, not yet posted. A reviewer no longer listed cannot be told
    // from a bot, so its pass is left unposted.
    fn posts_due(&self) -> Vec<Work> {
        let lineup = &self.contribution_lineup;
        (self.state.contributions.iter())
            .filter_map(|c| {
                let pass = c.pass.as_ref().filter(|p| p.head == c.head)?;
                let answered = pass.answered.as_ref()?;
                let reviewer = lineup.iter().find(|r| &r.name == answered)?;
                if reviewer.bot().is_some() {
                    return None;
                }
                let due = match (pass.findings.is_empty(), pass.review, pass.note) {
                    (false, None, _) => Due::Review(pass.findings.clone()),
                    (true, None, None) => Due::Note,
                    _ => return None,
                };
                let sent = self
                    .post_unsaved
                    .borrow()
                    .iter()
                    .any(|(n, h, _)| *n == c.pull_request && *h == c.head);
                if sent {
                    return None;
                }
                Some(Work {
                    number: c.pull_request,
                    head: c.head.clone(),
                    author: c.author.clone(),
                    due,
                })
            })
            .collect()
    }

    fn post_review(&mut self, work: &Work, findings: &[Finding]) -> Option<Posted> {
        if self.given_up(work, "review") {
            return None;
        }
        let worded = match self.kept_wording(work, "review") {
            Some(worded) => worded,
            None => {
                let worded = self.word_review(work, findings)?;
                self.keep_wording(work, "review", &worded);
                worded
            }
        };
        let sent = (self.ports.voice).review(
            &self.remote,
            work.number,
            &work.head,
            &worded.body,
            &worded.comments,
        );
        match sent {
            Ok(id) => {
                self.post_ends(work, "review");
                Some(Posted::Review(id))
            }
            Err(e) => {
                self.post_failed(work, "review", &e, !forge_is_busy(&e));
                if refused_text(&e) {
                    self.drop_wording(work, "review");
                }
                None
            }
        }
    }

    // The review as the representative words it, or in the findings' own
    // words when its answer is not the JSON asked for.
    fn word_review(&mut self, work: &Work, findings: &[Finding]) -> Option<Wording> {
        let shown = self.shown_lines(work);
        let (inline, aside): (Vec<&Finding>, Vec<&Finding>) = findings
            .iter()
            .partition(|f| f.line > 0 && shown.get(&f.file).is_some_and(|l| l.contains(&f.line)));
        let facts = review_facts(&work.head, &inline, &aside);
        let post = Post {
            pull_request: work.number,
            kind: REVIEW,
            facts: &facts,
            earlier: &[],
            contributor: &work.author,
            said: "",
        };
        let said = match self.represent(&post) {
            Ok(said) => said,
            Err(e) => {
                self.post_failed(work, "review", &e, true);
                return None;
            }
        };
        let mut words = read_words(&said, inline.len())
            .unwrap_or_else(|| own_words(&work.head, &inline, &aside, &work.author));
        words.body = with_aside(&words.body, &aside, &work.author);
        let comments = (inline.iter().zip(words.comments))
            .map(|(f, body)| LineComment {
                path: f.file.clone(),
                line: f.line,
                body,
            })
            .collect();
        Some(Wording {
            body: words.body,
            comments,
        })
    }

    fn post_note(&mut self, work: &Work) -> Option<Posted> {
        if self.given_up(work, "note") {
            return None;
        }
        let said = match self.kept_wording(work, "note") {
            Some(worded) => worded.body,
            None => {
                let said = self.word_note(work)?;
                let worded = Wording {
                    body: said.clone(),
                    comments: Vec::new(),
                };
                self.keep_wording(work, "note", &worded);
                said
            }
        };
        match (self.ports.voice).post(&self.remote, work.number, &said) {
            Ok(posted) => {
                self.post_ends(work, "note");
                Some(Posted::Note(posted.id))
            }
            Err(e) => {
                self.post_failed(work, "note", &e, !forge_is_busy(&e));
                if refused_text(&e) {
                    self.drop_wording(work, "note");
                }
                None
            }
        }
    }

    fn word_note(&mut self, work: &Work) -> Option<String> {
        let facts = format!(
            "The local reviewer read `{}` and found nothing to flag. The pull request is with \
             the maintainer now.",
            short(&work.head)
        );
        let post = Post {
            pull_request: work.number,
            kind: NOTE,
            facts: &facts,
            earlier: &[],
            contributor: &work.author,
            said: "",
        };
        match self.represent(&post) {
            Ok(said) => Some(said),
            Err(e) => {
                self.post_failed(work, "note", &e, true);
                None
            }
        }
    }

    // Keeps the id of what the forge took in the pass it was posted for.
    // A save that fails leaves the id to be kept at the next poll.
    fn keep_posted(&mut self, number: u64, head: &str, posted: Posted) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let held =
            (next.contributions.iter_mut()).find(|c| c.pull_request == number && c.head == head);
        let Some(pass) = held.and_then(|c| c.pass.as_mut()) else {
            return Ok(());
        };
        match posted {
            Posted::Review(id) => pass.review = Some(id),
            Posted::Note(id) => pass.note = Some(id),
        }
        let kept = self.save(next);
        if kept.is_err() {
            (self.post_unsaved.borrow_mut()).push((number, head.to_owned(), posted));
        }
        kept
    }

    // The lines of each file the head's diff shows, which a comment may be
    // put on. The head is read from the checkout the pass read. A diff that
    // cannot be read shows none, so every finding goes in the body.
    fn shown_lines(&self, work: &Work) -> BTreeMap<String, BTreeSet<u32>> {
        let worktree = (self.paths.folder.join(FOLDER)).join(work.number.to_string());
        let base = format!("origin/{}", self.settings.git.base());
        match diff_against(&self.settings.git.checkout, &worktree, &base) {
            Ok(diff) => lines_shown(&diff),
            Err(why) => {
                self.tell_diff(work, &why);
                BTreeMap::new()
            }
        }
    }

    // The wording an earlier poll made for the head, so a post that fails is
    // sent again without asking the representative again.
    fn kept_wording(&self, work: &Work, what: &str) -> Option<Wording> {
        let tries = self.post_tries.borrow();
        tries.get(&post_key(work, what))?.wording.clone()
    }

    fn keep_wording(&self, work: &Work, what: &str, worded: &Wording) {
        let mut tries = self.post_tries.borrow_mut();
        tries.entry(post_key(work, what)).or_default().wording = Some(worded.clone());
    }

    // The wording is worded again at the next try, since the text may have
    // been what the guard refused.
    fn drop_wording(&self, work: &Work, what: &str) {
        let mut tries = self.post_tries.borrow_mut();
        if let Some(attempt) = tries.get_mut(&post_key(work, what)) {
            attempt.wording = None;
        }
    }

    // Whether `what` failed [`POST_TRIES`] times for the head, which ends
    // the trying until the head changes or kelpie restarts.
    fn given_up(&self, work: &Work, what: &str) -> bool {
        let tries = self.post_tries.borrow();
        let failures = tries.get(&post_key(work, what)).map_or(0, |a| a.failures);
        failures >= POST_TRIES
    }

    // Says in the log that `what` could not be posted for the head, once, and
    // again when it is given up. A failure the forge being busy explains is
    // not counted, since asking again later may work.
    fn post_failed(&self, work: &Work, what: &str, why: &dyn std::fmt::Display, counts: bool) {
        let mut tries = self.post_tries.borrow_mut();
        let attempt = tries.entry(post_key(work, what)).or_default();
        let (number, head) = (work.number, short(&work.head));
        if counts {
            attempt.failures += 1;
        }
        if attempt.failures >= POST_TRIES {
            self.notes.push(format!(
                "contribution #{number}: cannot post the {what} on {head}: {why}; it failed \
                 {POST_TRIES} times, so it is not tried again for this head"
            ));
        } else if !attempt.told {
            attempt.told = true;
            self.notes.push(format!(
                "contribution #{number}: cannot post the {what} on {head}: {why}; it is tried at \
                 the next poll"
            ));
        }
    }

    // Says in the log, once for the head, that the diff could not be read.
    fn tell_diff(&self, work: &Work, why: &dyn std::fmt::Display) {
        let mut tries = self.post_tries.borrow_mut();
        let attempt = tries.entry(post_key(work, "diff")).or_default();
        if !std::mem::replace(&mut attempt.told, true) {
            self.notes.push(format!(
                "contribution #{}: cannot read the diff to place lines on {}: {why}",
                work.number,
                short(&work.head)
            ));
        }
    }

    // A post that worked leaves nothing to keep.
    fn post_ends(&self, work: &Work, what: &str) {
        self.post_tries.borrow_mut().remove(&post_key(work, what));
    }
}

fn post_key(work: &Work, what: &str) -> String {
    format!("{}:{}:{what}", work.number, work.head)
}

// Whether kelpie refused to send the text, as a hook or a local path does
pub(in crate::runner) fn refused_text(e: &ForgeError) -> bool {
    matches!(e, ForgeError::LocalPath { .. } | ForgeError::Hook { .. })
}

// Whether the forge could not be asked just now, so that asking again later
// may work
pub(in crate::runner) fn forge_is_busy(e: &ForgeError) -> bool {
    matches!(
        e,
        ForgeError::AppBusy(_) | ForgeError::Held(_) | ForgeError::Limited { .. }
    )
}

pub(super) fn short(head: &str) -> &str {
    head.get(..7).unwrap_or(head)
}

fn severity(s: Severity) -> &'static str {
    match s {
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
    }
}

// What a finding says, as the reviewer put it
fn said(f: &Finding) -> String {
    match f.why.trim().is_empty() {
        true => f.what.trim().to_owned(),
        false => format!("{}\n\n{}", f.what.trim(), f.why.trim()),
    }
}

// The facts the representative words a review from: the line findings in
// the order their comments come back, then the ones for the body
fn review_facts(head: &str, inline: &[&Finding], aside: &[&Finding]) -> String {
    let each = |f: &Finding| {
        let place = match f.line {
            0 => f.file.clone(),
            line => format!("{}:{line}", f.file),
        };
        let why = match f.why.trim().is_empty() {
            true => String::new(),
            false => format!(" Why it matters: {}", f.why.trim()),
        };
        format!("{}, {place}: {}.{why}", severity(f.severity), f.what.trim())
    };
    let mut facts = format!(
        "The local reviewer read `{}` and found {} things.\n",
        short(head),
        inline.len() + aside.len()
    );
    match inline.is_empty() {
        true => facts.push_str("\nNo finding has a line in the diff: \"comments\" is [].\n"),
        false => facts.push_str("\nLine findings, one comment each, in this order:\n"),
    }
    for (i, f) in inline.iter().enumerate() {
        facts.push_str(&format!("{}. {}\n", i + 1, each(f)));
    }
    if !aside.is_empty() {
        facts.push_str("\nFindings with no line in the diff, which kelpie adds under your body in their own words:\n");
        for f in aside {
            facts.push_str(&format!("- {}\n", each(f)));
        }
    }
    facts
}

// The representative's answer as the words of the review, or `None` when it
// is not the JSON asked for, has a blank body or a blank comment, or does not
// hold one comment for each of `count` line findings
fn read_words(said: &str, count: usize) -> Option<Words> {
    let (start, end) = (said.find('{')?, said.rfind('}')?);
    let words: Words = serde_json::from_str(said.get(start..=end)?).ok()?;
    let blank = |s: &String| s.trim().is_empty();
    let usable = !blank(&words.body) && words.comments.len() == count;
    (usable && !words.comments.iter().any(blank)).then_some(words)
}

// The review in the findings' own words, with every mention but the
// contributor's put in code as the representative's are
fn own_words(head: &str, inline: &[&Finding], aside: &[&Finding], contributor: &str) -> Words {
    let body = format!(
        "The local reviewer read `{}` and found {} things.",
        short(head),
        inline.len() + aside.len()
    );
    Words {
        body: neutered(&body, contributor),
        comments: (inline.iter())
            .map(|f| neutered(&said(f), contributor))
            .collect(),
    }
}

// `body` with each finding that has no line in the diff under it, in the
// finding's own words, so no wording of the representative can leave one out
fn with_aside(body: &str, aside: &[&Finding], contributor: &str) -> String {
    let mut body = body.to_owned();
    for f in aside {
        let place = match f.line {
            0 => f.file.clone(),
            line => format!("{}:{line}", f.file),
        };
        let text = neutered(&said(f), contributor);
        body.push_str(&format!("\n\n`{place}`: {text}"));
    }
    body
}

// The lines of the new file each file's hunks show, context included, which
// is where GitHub takes a comment on the right side of the diff. A hunk's
// header says how many lines it holds, so a changed line that reads like a
// file header is not taken for one.
fn lines_shown(diff: &str) -> BTreeMap<String, BTreeSet<u32>> {
    let mut shown: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
    let mut file: Option<String> = None;
    let (mut old_left, mut new_left, mut at) = (0u32, 0u32, 0u32);
    for line in diff.lines() {
        if old_left > 0 || new_left > 0 {
            let counted = !line.starts_with('\\');
            let (in_old, in_new) = match line.as_bytes().first() {
                Some(b'+') => (false, true),
                Some(b'-') => (true, false),
                _ => (true, true),
            };
            if counted && in_new {
                if let Some(file) = &file {
                    shown.entry(file.clone()).or_default().insert(at);
                }
                at += 1;
                new_left -= 1;
            }
            if counted && in_old {
                old_left = old_left.saturating_sub(1);
            }
        } else if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.split('\t').next().unwrap_or_default();
            file = path.strip_prefix("b/").map(str::to_owned);
        } else if let Some(hunk) = line.strip_prefix("@@ ")
            && let Some((old, new, start)) = hunk_header(hunk)
        {
            (old_left, new_left, at) = (old, new, start);
        }
    }
    shown
}

// The lengths of the old and new sides, and where the new one starts, from
// what follows `@@ ` in a hunk's header: `-12,3 +14,5 @@ ...`
fn hunk_header(hunk: &str) -> Option<(u32, u32, u32)> {
    let mut parts = hunk.split_whitespace();
    let side = |part: &str, sign: char| {
        let (start, len) = part
            .strip_prefix(sign)?
            .split_once(',')
            .unwrap_or((part.get(1..)?, "1"));
        Some((start.parse::<u32>().ok()?, len.parse::<u32>().ok()?))
    };
    let (_, old_len) = side(parts.next()?, '-')?;
    let (new_start, new_len) = side(parts.next()?, '+')?;
    Some((old_len, new_len, new_start))
}
