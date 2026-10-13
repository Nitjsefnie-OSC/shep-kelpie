//! Answering rulings from replies in their thread
//!
//! A ruling posted through kelpie's App that mentions someone says to reply
//! in its thread, the pull request or the issue it is about. Each
//! [`BOARD_POLL`] the runner asks the forge once for the repo's comments
//! since the last it read, and reads the new ones on the threads of the
//! rulings that wait. A reply counts when its author is the login the ruling
//! mentions and has write access to the repo: anyone else's, any bot's and
//! any comment kelpie itself posted, which is the maintainer's own login's
//! when it went through `gh`, is left alone, with no answer and no reaction.
//! A ruling that mentions no one is answered at the terminal only.
//!
//! The reply is read as `shep kelpie rule` reads an answer, with the ruling's
//! id first (see [`read_answer`]): `99 yes`, `99 no <note>` or `99 rework
//! <note>`. A bare `yes` answers nothing, so a conversational one never
//! merges. A worker's question takes any text, with the id first only when
//! several rulings wait on the thread. Kelpie reacts with a thumbs up to the
//! reply it took as the answer, and replies once to one it could not read,
//! saying what to type; the ruling stays open.
//!
//! A ruling's own comment is read too, for a reaction or a ticked box (see
//! [`marked`]). A 👍 or 👎 from the login the ruling mentions answers a yes or
//! a no; a merge ruling's and a question's options are task-list boxes, and a
//! tick counts only when every edit of the comment was by that login or by the
//! App that wrote it, which GraphQL's `userContentEdits` says. The answer is
//! applied as a reply's is, and the board's events tell the project manager.
//!
//! An answer given here and one given at the terminal or on ntfy: the first
//! wins. A ruling answered elsewhere is watched for a while longer, so a
//! reply here that names it is told who answered it and with what. Its own
//! comment is edited to say it is settled (see [`settled`]), unless the
//! maintainer answered it here.
//!
//! The forge is asked with the runner's lock let go: [`Runner::thread_job`]
//! decides a read is due and copies what it needs, [`ThreadJob::run`] reads,
//! and [`Runner::apply_threads`] acts on what was read.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use self::settled::Ended;
pub(super) use self::settled::{Outcome, Settler};
use super::looks::BOARD_POLL;
use super::trigger::lock;
use super::words::{Wants, read_answer};
use super::{Answer, RuleError, Runner, voice};
use crate::ports::{CommentMarks, Forge, ThreadComment, Timestamp};
use crate::settings::ForgeSlug;
use crate::state::{Heard, ProjectState, Ruling, RulingKind, StateError};
use crate::work_item::Phase;

/// Seconds a ruling answered elsewhere is still watched, so a reply that
/// came before the answer is told it is settled
const LATE: u64 = 60 * 60;

/// Seconds a ruling the panel decided is watched for a `no <note>`
const DECIDED: u64 = 24 * 60 * 60;

/// How far behind a read the next one reaches, in seconds, for a comment the
/// forge was slow to list
const SLACK: u64 = 5 * 60;

/// A ruling's thread being watched for an answer
#[derive(Debug, Clone)]
struct Watch {
    ruling: u64,
    thread: u64,
    heard: Heard,
    /// Until when it is watched after it was answered, once it was
    settled: Option<Timestamp>,
}

/// Where reading the rulings' threads stands, kept in memory only
#[derive(Debug, Default)]
pub(super) struct Reading {
    /// When the comments were last asked for, however that went
    read: Option<Timestamp>,
    /// The comments before this are not asked for again
    floor: Option<Timestamp>,
    watching: BTreeMap<u64, Watch>,
    /// The comments read and left alone, a bot's or a login's without write
    /// access, by id and when written, until a read no longer reaches them
    judged: BTreeMap<u64, Timestamp>,
    /// Why the last read failed, told once while it keeps failing
    failed: Option<String>,
    /// Why the last read of a ruling's comment for reactions and ticks
    /// failed, told once while it keeps failing
    marks_failed: Option<String>,
    /// Why a ruling's reaction or tick was refused, told once each, since
    /// it stays on the comment and is read again at every poll
    refused: BTreeMap<u64, String>,
    /// What became of each ruling that left the state while it was watched,
    /// as a reply in its thread is told, with the number it was read by
    told: BTreeMap<u64, (u64, String)>,
    /// The rulings the last save took out of the state while their comments
    /// were up, until what became of them is said
    ended: BTreeMap<u64, Ended>,
    /// The edit of a settled ruling's comment that failed, by ruling, tried
    /// again after a wait that doubles until it is given up
    unedited: BTreeMap<u64, settled::Unedited>,
    /// The rulings the panel decided while they are watched, as they stood
    /// when they were decided, so a `no <note>` can reopen one
    decided: BTreeMap<u64, Ruling>,
}

/// How a reply names the ruling it answers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    /// It names none
    Not,
    /// By the ruling's id
    Id,
    /// By the number of its issue or pull request
    Number,
}

/// What a comment on a watched thread is a reply to
enum Aimed {
    /// A pending ruling, named or not
    At(u64, Naming),
    /// A ruling that was answered already
    Settled(u64),
    /// A ruling pending on another thread
    Elsewhere(u64, u64),
    /// Several rulings wait on the thread and the reply names none
    Unnamed(Vec<u64>),
    /// A `no` that names no ruling, on the comment of one the panel decided
    Undecided(u64),
    /// Nothing to answer
    Nothing,
}

/// A read of the comments the runner decided was due, which needs no lock
pub(super) struct ThreadJob {
    forge: Arc<dyn Forge>,
    repo: ForgeSlug,
    since: Timestamp,
    watches: Vec<Watch>,
    judged: BTreeSet<u64>,
    own: BTreeSet<u64>,
}

/// What a read found
pub(super) struct ThreadRead {
    now: Timestamp,
    comments: Vec<ThreadComment>,
    /// Whether each author a reply could come from has write access
    writers: BTreeMap<String, bool>,
    /// The comment whose author's access could not be read, and why, where
    /// the read ended
    stopped: Option<(u64, String)>,
    /// What was done to each pending ruling's own comment
    marks: BTreeMap<u64, Result<CommentMarks, String>>,
}

/// Reads the rulings' threads for answers, if a read is due, asking the
/// forge with the runner's lock let go
pub(super) fn read_threads(runner: &Mutex<Runner>) -> Result<(), StateError> {
    let job = lock(runner).thread_job();
    let Some((now, job)) = job else {
        return Ok(());
    };
    let read = job.run(now);
    lock(runner).apply_threads(read)
}

impl ThreadJob {
    // Asks for the comments since the place read to, and who may write among
    // the authors of those that could be an answer.
    fn run(self, now: Timestamp) -> Result<ThreadRead, String> {
        let comments = (self.forge.comments_since(&self.repo, self.since))
            .map_err(|e| format!("cannot read the comments on the rulings' threads: {e}"))?;
        let mut writers = BTreeMap::new();
        let mut stopped = None;
        for comment in &comments {
            let aimed = (self.watches.iter()).any(|w| aims_at(w, comment));
            if comment.bot
                || !aimed
                || self.judged.contains(&comment.id)
                || self.own.contains(&comment.id)
                || writers.contains_key(&comment.author)
            {
                continue;
            }
            match self.forge.can_write(&self.repo, &comment.author) {
                Ok(may) => {
                    writers.insert(comment.author.clone(), may);
                }
                Err(e) => {
                    let who = &comment.author;
                    stopped = Some((comment.id, format!("cannot tell if {who} may write: {e}")));
                    break;
                }
            }
        }
        // A reaction or a tick on the ruling's own comment is read from the
        // comment itself, whatever was written in the thread since.
        let marks = (self.watches.iter())
            .filter(|w| w.settled.is_none() && w.heard.ruling_comment != 0)
            .map(|w| (w.ruling, w.heard.ruling_comment))
            .map(|(ruling, comment)| {
                let marks = (self.forge.comment_marks(&self.repo, comment))
                    .map_err(|e| format!("cannot read the reactions on comment {comment}: {e}"));
                (ruling, marks)
            })
            .collect();
        Ok(ThreadRead {
            now,
            comments,
            writers,
            stopped,
            marks,
        })
    }
}

impl Runner {
    // Decides whether the threads are read now: at most once a [`BOARD_POLL`],
    // where kelpie's App covers the repo and a ruling waits in its thread.
    // Returns the time and what the read needs.
    fn thread_job(&mut self) -> Option<(Timestamp, ThreadJob)> {
        let now = self.ports.clock.now();
        self.watch_threads(now);
        if self.threads.watching.is_empty() {
            return None;
        }
        let poll = BOARD_POLL.as_secs();
        if (self.threads.read).is_some_and(|at| now.0 < at.0.saturating_add(poll)) {
            return None;
        }
        if !self.app_covers() {
            return None;
        }
        self.threads.read = Some(now);
        let watches: Vec<Watch> = self.threads.watching.values().cloned().collect();
        let heard = watches.iter().map(|w| w.heard.time).min();
        let since = heard.into_iter().chain(self.threads.floor).max();
        let mut own: BTreeSet<u64> = self.state.posted.iter().copied().collect();
        own.extend(self.posted.borrow().iter().copied());
        let job = ThreadJob {
            forge: Arc::clone(&self.ports.forge),
            repo: self.remote.clone(),
            since: since.unwrap_or(now),
            watches,
            judged: self.threads.judged.keys().copied().collect(),
            own,
        };
        Some((now, job))
    }

    // Acts on what a read found: the new replies by the login a ruling
    // mentions, with write access.
    fn apply_threads(&mut self, read: Result<ThreadRead, String>) -> Result<(), StateError> {
        let read = match read {
            Ok(read) => read,
            Err(why) => {
                if self.threads.failed.replace(why.clone()).as_deref() != Some(why.as_str()) {
                    self.notes.push(why);
                }
                return Ok(());
            }
        };
        self.threads.failed = None;
        for comment in &read.comments {
            if (read.stopped.as_ref()).is_some_and(|(id, _)| *id == comment.id) {
                let why = read.stopped.as_ref().map(|(_, why)| why.clone());
                self.notes.extend(why);
                return Ok(());
            }
            let aimed = self.threads.aimed_by(comment);
            if aimed.is_empty() || self.threads.judged.contains_key(&comment.id) {
                continue;
            }
            // The maintainer's own login wrote it for kelpie, not as an answer.
            let own = self.state.posted.contains(&comment.id)
                || self.posted.borrow().contains(&comment.id);
            if comment.bot || own {
                self.threads.judged.insert(comment.id, comment.created);
                continue;
            }
            // An author the read did not ask about is left for the next one.
            let Some(&may) = read.writers.get(&comment.author) else {
                continue;
            };
            if may {
                // Before it is acted on, so a reply is answered once. Only a
                // reply acted on moves the place read to on its thread: any
                // other comment moving it would skip a reply the forge lists
                // later.
                self.hear(comment)?;
                self.respond(comment, &aimed);
            } else {
                self.threads.judged.insert(comment.id, comment.created);
            }
        }
        let floor = Timestamp(read.now.0.saturating_sub(SLACK));
        self.threads.floor = Some(floor);
        self.threads.judged.retain(|_, created| *created >= floor);
        self.apply_marks(read.marks);
        Ok(())
    }

    // Answers each ruling its comment's reaction or ticked box answers, when
    // the maintainer it mentions gave it. Their reaction stays on the comment
    // and is read at every poll, so a refusal is told once.
    fn apply_marks(&mut self, marks: BTreeMap<u64, Result<CommentMarks, String>>) {
        let mut failed = Vec::new();
        for (id, marks) in marks {
            let marks = match marks {
                Ok(marks) => marks,
                Err(why) => {
                    failed.push(why);
                    continue;
                }
            };
            let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
                continue;
            };
            let Some(watch) = self.threads.watching.get(&id) else {
                continue;
            };
            let Some(answer) = marked(&ruling.kind, &watch.heard.mentions, &marks) else {
                continue;
            };
            match self.rule_by(id, answer, Settler::Thread) {
                Ok(()) => {
                    self.threads.refused.remove(&id);
                }
                Err(e) => {
                    let why =
                        format!("cannot answer ruling {id} from the reaction on its comment: {e}");
                    if self.threads.refused.insert(id, why.clone()).as_deref() != Some(&why) {
                        self.notes.push(why);
                    }
                }
            }
        }
        // Every comment that could not be read, told together and only when
        // that set changes.
        let failed = (!failed.is_empty()).then(|| failed.join("; "));
        if self.threads.marks_failed != failed {
            self.notes.extend(failed.clone());
            self.threads.marks_failed = failed;
        }
    }

    // Watches each pending ruling's thread, and a ruling that was answered
    // for a while after.
    fn watch_threads(&mut self, now: Timestamp) {
        let state = &self.state;
        let open: Vec<_> = (state.rulings.iter())
            .filter_map(|ruling| watch_of(state, ruling))
            .collect();
        let threads = &mut self.threads.watching;
        for (id, watch) in &open {
            // The place read to is kept in memory too, but the state's wins.
            threads.insert(*id, watch.clone());
        }
        threads.retain(|id, watch| {
            if open.iter().any(|(open, _)| open == id) {
                return true;
            }
            let until = *watch
                .settled
                .get_or_insert(Timestamp(now.0.saturating_add(LATE)));
            now < until
        });
        self.threads.told.retain(|id, _| threads.contains_key(id));
        self.threads
            .decided
            .retain(|id, _| threads.contains_key(id));
        // What a ruling's end said is said by the call that ended it, before
        // any poll, so a ruling still here at a poll ended another way.
        self.threads.ended.clear();
        self.retry_edits(now);
    }

    // Moves the place read to forward to `comment` for each ruling on its
    // thread, and saves it with each pending ruling.
    fn hear(&mut self, comment: &ThreadComment) -> Result<(), StateError> {
        let mut moved = false;
        for watch in self.threads.watching.values_mut() {
            if watch.thread == comment.thread && watch.heard.before(comment.created, comment.id) {
                watch.heard.time = comment.created;
                watch.heard.comment = comment.id;
                moved = true;
            }
        }
        if !moved {
            return Ok(());
        }
        let mut next = self.state.clone();
        // The comment is the ruling's answer, not a request of the worker.
        if let Some(item) = next
            .work_items
            .iter_mut()
            .find(|i| i.pull_request == Some(comment.thread))
        {
            item.talked.hear(comment.id);
        }
        for ruling in &self.state.rulings {
            if let Some(watch) = self.threads.watching.get(&ruling.id) {
                next.heard.insert(ruling.id, watch.heard.clone());
            }
        }
        self.save(next)
    }

    // Acts on a reply by the login a ruling mentions, with write access.
    fn respond(&mut self, comment: &ThreadComment, aimed: &[u64]) {
        let words = comment.body.trim();
        let (named, words) = self.threads.name_of(words, comment, &self.state.rulings);
        let open = |id: &u64| self.state.rulings.iter().any(|r| r.id == *id);
        let waiting: Vec<u64> = aimed.iter().copied().filter(open).collect();
        let aimed = match named {
            Some((id, naming)) if waiting.contains(&id) => Aimed::At(id, naming),
            Some((id, _)) => match self.threads.watching.get(&id) {
                Some(Watch {
                    settled: Some(_), ..
                }) => Aimed::Settled(id),
                Some(Watch { thread, .. }) => Aimed::Elsewhere(id, *thread),
                None => Aimed::Nothing,
            },
            None => match waiting.as_slice() {
                [id] => Aimed::At(*id, Naming::Not),
                [] => match self.undecided(aimed, words) {
                    Some(id) => Aimed::Undecided(id),
                    None => Aimed::Nothing,
                },
                several => Aimed::Unnamed(several.to_vec()),
            },
        };
        let who = &comment.author;
        let thread = comment.thread;
        match aimed {
            Aimed::Nothing => {}
            Aimed::At(id, naming) => self.answer(comment, id, naming, words),
            Aimed::Settled(id) if self.reopens(comment, id, words) => {}
            Aimed::Settled(id) => {
                let (number, told) = match self.threads.told.get(&id) {
                    Some((number, told)) => (*number, told.as_str()),
                    None => (self.ruling_number(id), "is already answered"),
                };
                let said = format!("@{who} Ruling #{number} {told}.");
                self.say(thread, &said);
            }
            Aimed::Undecided(id) => {
                let number = self.ruling_number(id);
                let said = format!(
                    "@{who} Ruling #{number} was not reopened: start your reply with its \
                     number, as `{number} no <note>`."
                );
                self.say(thread, &said);
            }
            Aimed::Elsewhere(id, there) => {
                let number = self.ruling_number(id);
                let said =
                    format!("@{who} Ruling #{number} waits on #{there}, so answer it there.");
                self.say(thread, &said);
            }
            // Rulings on one thread share its number, so only an older kelpie
            // leaves several here, and only their ids tell them apart.
            Aimed::Unnamed(ids) => {
                let ids: Vec<_> = ids.iter().map(u64::to_string).collect();
                let said = format!(
                    "@{who} Rulings {} wait on this thread, so start your reply with the id of \
                     the one you answer, such as `{} yes`.",
                    ids.join(", "),
                    ids[0]
                );
                self.say(thread, &said);
            }
        }
    }

    // The number ruling `id` is read by, or `id` once it has left the state
    // and the panel's decided rulings
    pub(super) fn ruling_number(&self, id: u64) -> u64 {
        let pending = self.state.rulings.iter().find(|r| r.id == id);
        let ended = self.threads.ended.get(&id).map(|e| &e.ruling);
        (pending.or(self.threads.decided.get(&id)).or(ended)).map_or(id, Ruling::number)
    }

    // The ruling the panel decided that `words`, a `no` naming none, reply to.
    fn undecided(&self, aimed: &[u64], words: &str) -> Option<u64> {
        let first = words.split_whitespace().next()?;
        if !first.eq_ignore_ascii_case("no") {
            return None;
        }
        (aimed.iter().copied()).find(|id| self.threads.decided.contains_key(id))
    }

    // Answers ruling `id` with `words`, reacting if it took, or says why not.
    // A ruling that takes a yes or a no is answered only by a reply that
    // names it, so a conversational yes never merges.
    fn answer(&mut self, comment: &ThreadComment, id: u64, naming: Naming, words: &str) {
        let (who, thread) = (&comment.author, comment.thread);
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let wants = Wants::of(&ruling.kind);
        let stale = naming == Naming::Number
            && ruling.raised.is_some_and(|raised| comment.created < raised);
        let ruled = if stale {
            let asks = ruling.question.lines().next().unwrap_or_default();
            Err(format!(
                "the ruling changed since that comment was written, and it asks now: {asks}"
            ))
        } else if naming != Naming::Not || wants == Wants::Answer {
            read_answer(wants, words).and_then(|answer| {
                (self.rule_by(id, answer, Settler::Thread)).map_err(|e| e.to_string())
            })
        } else {
            Err(format!(
                "start your reply with its number, as {}",
                forms(wants, ruling.number())
            ))
        };
        let said = match ruled {
            Ok(()) => {
                if let Err(e) = self.ports.voice.thumbs_up(&self.remote, comment.id) {
                    self.notes
                        .push(format!("cannot react to {who}'s answer on #{thread}: {e}"));
                }
                return;
            }
            Err(why) => {
                let number = self.ruling_number(id);
                format!(
                    "@{who} Ruling #{number} was not answered: {why}. Reply again in this \
                     thread, starting with `{number}`."
                )
            }
        };
        self.say(thread, &said);
    }

    /// What became of ruling `id` as a reply in its thread is told, if it ended
    pub(super) fn threads_told(&self, id: u64) -> Option<&String> {
        self.threads.told.get(&id).map(|(_, told)| told)
    }

    /// Watches the comment that posted the panel's decision on ruling
    /// `decided`, so `login`'s `no <note>` in `thread` reopens it
    pub(super) fn watch_decided(
        &mut self,
        decided: Ruling,
        thread: u64,
        login: &str,
        posted: crate::ports::PostedComment,
    ) {
        let until = Timestamp(self.ports.clock.now().0.saturating_add(DECIDED));
        let id = decided.id;
        let heard = Heard {
            mentions: login.to_owned(),
            time: posted.created,
            comment: posted.id,
            ruling_comment: 0,
        };
        let watch = Watch {
            ruling: id,
            thread,
            heard,
            settled: Some(until),
        };
        self.threads.watching.insert(id, watch);
        self.threads.decided.insert(id, decided);
    }

    // Reopens a ruling the panel decided when `words`, a reply to it, are a
    // `no <note>`: it is raised again as it stood, its work item is parked on
    // it again, and the note answers it. Says why when it cannot, and
    // returns whether the ruling was one the panel decided.
    fn reopens(&mut self, comment: &ThreadComment, id: u64, words: &str) -> bool {
        let Some(ruling) = self.threads.decided.get(&id).cloned() else {
            return false;
        };
        let (who, thread) = (&comment.author, comment.thread);
        // A worker's question takes text, so its `no <note>` sends the note.
        let answer = match Wants::of(&ruling.kind) {
            Wants::Answer => (no_note(words).map(Answer::Text))
                .ok_or_else(|| "only a `no <note>` reopens it".to_owned()),
            wants => read_answer(wants, words),
        };
        let reopened = match answer {
            Ok(answer @ (Answer::No(_) | Answer::Rework(_) | Answer::Text(_))) => {
                self.reopen(ruling, answer).map_err(|e| e.to_string())
            }
            Ok(_) => Err("only a `no <note>` reopens it".to_owned()),
            Err(why) => Err(why),
        };
        match reopened {
            Ok(()) => {
                self.threads.decided.remove(&id);
                if let Err(e) = self.ports.voice.thumbs_up(&self.remote, comment.id) {
                    self.notes
                        .push(format!("cannot react to {who}'s answer on #{thread}: {e}"));
                }
            }
            Err(why) => {
                let number = self.ruling_number(id);
                let said = format!("@{who} Ruling #{number} was not reopened: {why}.");
                self.say(thread, &said);
            }
        }
        true
    }

    // Puts `ruling` back where its work item waits on it, and answers it.
    fn reopen(&mut self, ruling: Ruling, answer: Answer) -> Result<(), RuleError> {
        let id = ruling.id;
        let not = |why: &str| RuleError::Reopen(id, why.to_owned());
        let issue = ruling.issue.ok_or_else(|| not("it has no work item"))?;
        let item = self.state.item(issue);
        let item = item.ok_or_else(|| not("its work item has finished"))?;
        if self.flights.flying(issue) {
            return Err(not("its worker is running"));
        }
        if matches!(item.phase, Phase::Ruling { .. } | Phase::Done { .. }) {
            return Err(not("its work item waits on another ruling or is done"));
        }
        let before = self.state.clone();
        let mut next = before.clone();
        if let Some(item) = next.item_mut(issue) {
            item.phase = Phase::Ruling { id };
        }
        next.rulings.push(ruling);
        self.save(next).map_err(RuleError::State)?;
        let ruled = self.rule_by(id, answer, Settler::Thread);
        if ruled.is_err() {
            // The ruling stays pending when its answer fails, so put the
            // work item back as it was: the reply says it was not reopened.
            if let Err(e) = self.save(before) {
                self.notes.push(format!(
                    "cannot put ruling {id} back after a failed reopen: {e}"
                ));
            }
        }
        ruled
    }

    // Posts `text` on `thread` as the App, and notes why it could not.
    fn say(&mut self, thread: u64, text: &str) {
        if let Err(e) = self.ports.voice.comment(&self.remote, thread, text) {
            self.notes.push(format!("cannot reply on #{thread}: {e}"));
        }
    }
}

// The note of a `no <note>`, if `words` are one.
fn no_note(words: &str) -> Option<String> {
    let (first, note) = words.trim().split_once(char::is_whitespace)?;
    let note = note.trim();
    (first.eq_ignore_ascii_case("no") && !note.is_empty()).then(|| note.to_owned())
}

impl Reading {
    // Keeps watching each ruling that `before` had and `after` has not,
    // however it was answered, for a while.
    pub(super) fn settled(&mut self, before: &ProjectState, after: &ProjectState, now: Timestamp) {
        let until = Timestamp(now.0.saturating_add(LATE));
        let gone = (before.rulings.iter()).filter(|r| !after.rulings.iter().any(|a| a.id == r.id));
        for ruling in gone {
            // A ruling posted with no mention has no place read to, only its
            // comment's id.
            let kept = before.heard.get(&ruling.id).map(|h| h.ruling_comment);
            let comment = kept
                .filter(|comment| *comment != 0)
                .or_else(|| before.unmentioned.get(&ruling.id).copied());
            if let Some(comment) = comment {
                let ruling = ruling.clone();
                self.ended.insert(ruling.id, Ended { ruling, comment });
            }
            if let Some((id, watch)) = watch_of(before, ruling) {
                let watch = self.watching.entry(id).or_insert(watch);
                watch.settled.get_or_insert(until);
            }
        }
    }

    // The watched rulings `comment` is new to: on their thread, written after
    // the place read to, by the login each mentions.
    fn aimed_by(&self, comment: &ThreadComment) -> Vec<u64> {
        (self.watching.iter())
            .filter(|(_, w)| aims_at(w, comment))
            .map(|(&id, _)| id)
            .collect()
    }

    // The id of a ruling a reply starts with, and the rest of it. The number
    // is the number of the issue or pull request of the one ruling in
    // `rulings` this author may answer, since that is what the card prints, or
    // failing that the id of a ruling, waiting or settled lately, so a late
    // answer is told so. A number that names no ruling this author may
    // answer, or the whole of a reply, is its text.
    fn name_of<'a>(
        &self,
        words: &'a str,
        comment: &ThreadComment,
        rulings: &[Ruling],
    ) -> (Option<(u64, Naming)>, &'a str) {
        let Some((first, rest)) = words.split_once(char::is_whitespace) else {
            return (None, words);
        };
        let Ok(number) = first.trim_start_matches('#').parse::<u64>() else {
            return (None, words);
        };
        if rest.trim().is_empty() {
            return (None, words);
        }
        let mentioned = |id: &u64| {
            (self.watching.get(id)).is_some_and(|w| w.heard.is_mentioned(&comment.author))
        };
        let rest = rest.trim_start();
        let about = |r: &&Ruling| r.issue == Some(number) || r.pull_request == Some(number);
        // A ruling that takes any text is answered by a reply that names no
        // ruling when it is the only one waiting, so a number opening the
        // reply is part of the answer, not its name.
        let named_by_number = |r: &&Ruling| Wants::of(&r.kind) != Wants::Answer;
        let mut about = (rulings.iter())
            .filter(about)
            .filter(named_by_number)
            .filter(|r| mentioned(&r.id));
        match (about.next(), about.next()) {
            (Some(ruling), None) => (Some((ruling.id, Naming::Number)), rest),
            _ if mentioned(&number) => (Some((number, Naming::Id)), rest),
            _ => (None, words),
        }
    }
}

// Whether `comment` is new to the ruling `watch` follows, and from the login
// it mentions.
fn aims_at(watch: &Watch, comment: &ThreadComment) -> bool {
    watch.thread == comment.thread
        && watch.heard.before(comment.created, comment.id)
        && watch.heard.is_mentioned(&comment.author)
}

// The answer a ruling's comment gives of itself: the thumbs the login it
// mentions left, else a box ticked in it. A thumbs up with a thumbs down
// answers nothing. A tick counts only when every edit of the comment was made
// by that login or by the App that wrote it, since anyone with write access
// can edit a comment, and the forge does not say which edit ticked the box.
fn marked(kind: &RulingKind, maintainer: &str, marks: &CommentMarks) -> Option<Answer> {
    let thumbs = |up: bool| {
        (marks.thumbs.iter())
            .any(|thumb| thumb.up == up && thumb.login.eq_ignore_ascii_case(maintainer))
    };
    match (thumbs(true), thumbs(false)) {
        (true, true) => return None,
        (true, false) => {
            if let Some(answer) = voice::thumbs(kind, true) {
                return Some(answer);
            }
        }
        (false, true) => {
            if let Some(answer) = voice::thumbs(kind, false) {
                return Some(answer);
            }
        }
        (false, false) => {}
    }
    let own = |login: &str| plain_login(login) == plain_login(&marks.author);
    let theirs = |login: &String| login.eq_ignore_ascii_case(maintainer) || own(login);
    let edited = !marks.editors.is_empty() && marks.editors.iter().all(theirs);
    edited.then(|| voice::ticked(kind, &marks.body)).flatten()
}

// GraphQL names an App's login without the `[bot]` REST adds.
fn plain_login(login: &str) -> String {
    let login = login.strip_suffix("[bot]").unwrap_or(login);
    login.to_ascii_lowercase()
}

// What a reply to a ruling that wants `wants` starts with: its number.
fn forms(wants: Wants, id: u64) -> String {
    match wants {
        Wants::Answer => format!("`{id} <your answer>`"),
        Wants::Merge => format!("`{id} yes`, `{id} no <note>` or `{id} rework <note>`"),
        Wants::YesOrNo => format!("`{id} yes` or `{id} no <note>`"),
    }
}

// The thread a pending ruling is answered in, and the place read to there.
// A ruling kelpie's App did not post with a mention is answered at the
// terminal only, so it has none.
fn watch_of(state: &ProjectState, ruling: &Ruling) -> Option<(u64, Watch)> {
    let thread = ruling.pull_request.or(ruling.issue)?;
    let heard = state.heard.get(&ruling.id)?.clone();
    let settled = None;
    Some((
        ruling.id,
        Watch {
            ruling: ruling.id,
            thread,
            heard,
            settled,
        },
    ))
}

mod settled;

#[cfg(test)]
mod tests;
