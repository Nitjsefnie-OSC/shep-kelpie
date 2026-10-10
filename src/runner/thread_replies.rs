//! Answering rulings from replies in their thread
//!
//! A ruling posted through kelpie's App says to reply in its thread, the
//! pull request or the issue it is about. Each [`BOARD_POLL`] the runner
//! asks the forge once for the repo's comments since the last it read, and
//! reads the new ones on the threads of the rulings that wait. A reply counts
//! when its author has write access to the repo: anyone else's, and any
//! bot's, is left alone, with no answer and no reaction.
//!
//! The reply is read as `shep kelpie rule` reads an answer, without the id
//! (see [`read_answer`]): `yes`, `no <note>`, `rework <note>`, or the free
//! text a question takes. When several rulings wait on one thread the reply
//! starts with the id of the one it answers. Kelpie reacts with a thumbs up
//! to the reply it took as the answer, and replies once to one it could not
//! read, saying what the ruling takes; the ruling stays open.
//!
//! An answer given here and one given at the terminal or on ntfy: the first
//! wins. A ruling answered elsewhere is watched for a while longer, so a
//! reply here that reads as an answer is told it is already answered.

use std::collections::BTreeMap;

use super::Runner;
use super::looks::BOARD_POLL;
use super::ruling::Answer;
use super::words::{Wants, read_answer};
use crate::ports::{ThreadComment, Timestamp};
use crate::state::{Heard, ProjectState, Ruling, StateError};

/// Seconds a ruling answered elsewhere is still watched, so a reply that
/// came before the answer is told it is settled
const LATE: u64 = 60 * 60;

/// How far behind a read the next one reaches, in seconds, for a comment the
/// forge was slow to list
const SLACK: u64 = 5 * 60;

/// A ruling's thread being watched for an answer
#[derive(Debug, Clone, Copy)]
struct Watch {
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
    /// Why the last read failed, told once while it keeps failing
    failed: Option<String>,
}

/// What a comment on a watched thread is a reply to
enum Aimed {
    /// A pending ruling
    At(u64),
    /// A ruling that was answered already
    Settled(u64),
    /// A ruling pending on another thread
    Elsewhere(u64, u64),
    /// Several rulings wait on the thread and the reply names none
    Unnamed(Vec<u64>),
    /// Nothing to answer
    Nothing,
}

impl Runner {
    /// Reads the new comments on the threads of the rulings that wait, at
    /// most once a [`BOARD_POLL`], and answers those they answer
    ///
    /// Only where kelpie's App covers the repo. A forge that cannot be asked
    /// is noted, once while it keeps failing, and asked again at the next
    /// poll.
    ///
    /// # Errors
    ///
    /// [`StateError::Write`] when the place read to cannot be saved.
    pub(super) fn read_threads(&mut self) -> Result<(), StateError> {
        let now = self.ports.clock.now();
        self.begin_hearing(now)?;
        self.watch_threads(now);
        if self.threads.watching.is_empty() {
            return Ok(());
        }
        let poll = BOARD_POLL.as_secs();
        if (self.threads.read).is_some_and(|at| now.0 < at.0.saturating_add(poll)) {
            return Ok(());
        }
        if !self.app_covers() {
            return Ok(());
        }
        self.threads.read = Some(now);
        let heard = self.threads.watching.values().map(|w| w.heard.time).min();
        let since = heard.into_iter().chain(self.threads.floor).max();
        let comments = match self
            .ports
            .forge
            .comments_since(&self.remote, since.unwrap_or(now))
        {
            Ok(comments) => comments,
            Err(e) => {
                let why = format!("cannot read the comments on the rulings' threads: {e}");
                if self.threads.failed.replace(why.clone()).as_deref() != Some(why.as_str()) {
                    self.notes.push(why);
                }
                return Ok(());
            }
        };
        self.threads.failed = None;
        let mut writers = BTreeMap::new();
        for comment in &comments {
            let aimed = self.threads.aimed_by(comment);
            if comment.bot || aimed.is_empty() {
                continue;
            }
            let may = match writers.get(&comment.author) {
                Some(&may) => may,
                None => match self.ports.forge.can_write(&self.remote, &comment.author) {
                    Ok(may) => *writers.entry(comment.author.clone()).or_insert(may),
                    Err(e) => {
                        let who = &comment.author;
                        self.notes
                            .push(format!("cannot tell if {who} may write: {e}"));
                        return Ok(());
                    }
                },
            };
            if may {
                // Before it is acted on, so a reply is answered once.
                self.hear(comment.created, comment.id)?;
                self.respond(comment, &aimed);
            }
        }
        if let Some(last) = comments.last() {
            self.hear(last.created, last.id)?;
        }
        self.threads.floor = Some(Timestamp(now.0.saturating_sub(SLACK)));
        Ok(())
    }

    // Starts the reading of a ruling saved before replies in a thread, from now.
    fn begin_hearing(&mut self, now: Timestamp) -> Result<(), StateError> {
        let unheard: Vec<u64> = (self.state.rulings.iter())
            .map(|r| r.id)
            .filter(|id| !self.state.heard.contains_key(id))
            .collect();
        if unheard.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        for id in unheard {
            next.heard.insert(id, Heard::at(now));
        }
        self.save(next)
    }

    // Watches each pending ruling's thread, and a ruling that was answered
    // for a while after.
    fn watch_threads(&mut self, now: Timestamp) {
        let threads = &mut self.threads.watching;
        let state = &self.state;
        let open: Vec<_> = (state.rulings.iter())
            .filter_map(|ruling| watch_of(state, ruling))
            .collect();
        for (id, watch) in &open {
            threads.insert(*id, *watch);
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
    }

    // Moves the place read to forward to the comment written at `time` with
    // `id`, and saves it with each pending ruling.
    fn hear(&mut self, time: Timestamp, id: u64) -> Result<(), StateError> {
        let mut moved = false;
        for watch in self.threads.watching.values_mut() {
            if watch.heard.before(time, id) {
                watch.heard = Heard { time, comment: id };
                moved = true;
            }
        }
        if !moved {
            return Ok(());
        }
        let mut next = self.state.clone();
        for ruling in &self.state.rulings {
            if let Some(watch) = self.threads.watching.get(&ruling.id) {
                next.heard.insert(ruling.id, watch.heard);
            }
        }
        self.save(next)
    }

    // Acts on a reply by a login with write access.
    fn respond(&mut self, comment: &ThreadComment, aimed: &[u64]) {
        let words = comment.body.trim();
        let (named, words) = self.threads.name_of(words);
        let open = |id: &u64| self.state.rulings.iter().any(|r| r.id == *id);
        let waiting: Vec<u64> = aimed.iter().copied().filter(open).collect();
        let aimed = match named {
            Some(id) if waiting.contains(&id) => Aimed::At(id),
            Some(id) => match self.threads.watching.get(&id) {
                Some(Watch {
                    settled: Some(_), ..
                }) => Aimed::Settled(id),
                Some(Watch { thread, .. }) => Aimed::Elsewhere(id, *thread),
                None => Aimed::Nothing,
            },
            None => match waiting.as_slice() {
                [id] => Aimed::At(*id),
                [] => {
                    let settled = aimed.iter().find(|id| !open(id));
                    let first = words
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    match settled {
                        Some(&id) if ["yes", "no", "rework"].contains(&first.as_str()) => {
                            Aimed::Settled(id)
                        }
                        _ => Aimed::Nothing,
                    }
                }
                several => Aimed::Unnamed(several.to_vec()),
            },
        };
        let who = &comment.author;
        let thread = comment.thread;
        match aimed {
            Aimed::Nothing => {}
            Aimed::At(id) => self.answer(comment, id, words),
            Aimed::Settled(id) => {
                let said = format!("@{who} Ruling {id} is already answered.");
                self.say(thread, &said);
            }
            Aimed::Elsewhere(id, there) => {
                let said = format!("@{who} Ruling {id} waits on #{there}, so answer it there.");
                self.say(thread, &said);
            }
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

    // Answers ruling `id` with `words`, reacting if it took, or says why not.
    fn answer(&mut self, comment: &ThreadComment, id: u64, words: &str) {
        let (who, thread) = (&comment.author, comment.thread);
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return;
        };
        let answer: Result<Answer, String> = read_answer(Wants::of(&ruling.kind), words);
        let ruled = answer.and_then(|answer| self.rule(id, answer).map_err(|e| e.to_string()));
        let said = match ruled {
            Ok(()) => {
                if let Err(e) = self.ports.voice.thumbs_up(&self.remote, comment.id) {
                    self.notes
                        .push(format!("cannot react to {who}'s answer on #{thread}: {e}"));
                }
                return;
            }
            Err(why) => format!(
                "@{who} Ruling {id} was not answered: {why}. Reply again in this thread, \
                 starting with `{id}` if other rulings wait on it."
            ),
        };
        self.say(thread, &said);
    }

    // Posts `text` on `thread` as the App, and notes why it could not.
    fn say(&mut self, thread: u64, text: &str) {
        if let Err(e) = self.ports.voice.comment(&self.remote, thread, text) {
            self.notes.push(format!("cannot reply on #{thread}: {e}"));
        }
    }
}

impl Reading {
    // Keeps watching each ruling that `before` had and `after` has not,
    // however it was answered, for a while.
    pub(super) fn settled(&mut self, before: &ProjectState, after: &ProjectState, now: Timestamp) {
        let until = Timestamp(now.0.saturating_add(LATE));
        let gone = before
            .rulings
            .iter()
            .filter(|r| !after.rulings.iter().any(|a| a.id == r.id));
        for (id, watch) in gone.filter_map(|ruling| watch_of(before, ruling)) {
            let watch = self.watching.entry(id).or_insert(watch);
            watch.settled.get_or_insert(until);
        }
    }

    // The watched rulings `comment` is new to: on their thread, and written
    // after the place read to.
    fn aimed_by(&self, comment: &ThreadComment) -> Vec<u64> {
        (self.watching.iter())
            .filter(|(_, w)| {
                w.thread == comment.thread && w.heard.before(comment.created, comment.id)
            })
            .map(|(&id, _)| id)
            .collect()
    }

    // The id of a ruling a reply starts with, and the rest of it. A number
    // that names no ruling watched, or the whole of a reply, is its text.
    fn name_of<'a>(&self, words: &'a str) -> (Option<u64>, &'a str) {
        let Some((first, rest)) = words.split_once(char::is_whitespace) else {
            return (None, words);
        };
        let id = first.trim_start_matches('#').parse().ok();
        match id {
            Some(id) if self.watching.contains_key(&id) && !rest.trim().is_empty() => {
                (Some(id), rest.trim_start())
            }
            _ => (None, words),
        }
    }
}

// The thread a pending ruling is answered in, and the place read to there.
fn watch_of(state: &ProjectState, ruling: &Ruling) -> Option<(u64, Watch)> {
    let thread = ruling.pull_request.or(ruling.issue)?;
    let heard = *state.heard.get(&ruling.id)?;
    let settled = None;
    Some((
        ruling.id,
        Watch {
            thread,
            heard,
            settled,
        },
    ))
}

#[cfg(test)]
mod tests;
