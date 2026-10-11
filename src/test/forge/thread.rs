//! The comments on the fake forge's threads, and who may write to its repo

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::ports::{Clock, CommentMarks, ThreadComment, Thumb, Timestamp};
use crate::test::FakeClock;

/// The first id a comment on the fake forge gets
const FIRST: u64 = 5000;

#[derive(Debug, Default)]
struct Threads {
    comments: Vec<ThreadComment>,
    writers: HashSet<String>,
    // The `since` of each read of the repo's comments
    reads: Vec<Timestamp>,
    // The login of each ask whether it may write
    asked: Vec<String>,
    // What people did to the comments kelpie posted, by comment id
    marks: HashMap<u64, CommentMarks>,
    // The comments whose marks cannot be read
    unreadable: HashSet<u64>,
}

/// What people wrote on the repo's issues and pull requests
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeThreads {
    threads: Arc<Mutex<Threads>>,
    // The clock the comments kelpie posts as its own login are written by
    clock: Arc<Mutex<Option<FakeClock>>>,
}

impl FakeThreads {
    /// Has the comments kelpie posts as its own login written by `clock`
    pub(crate) fn use_clock(&self, clock: FakeClock) {
        *self.clock.lock().unwrap() = Some(clock);
    }

    /// `author` writes `body` on `thread` now, by the clock given, and the comment's id
    pub(super) fn write_now(&self, thread: u64, author: &str, body: &str) -> u64 {
        let now = (self.clock.lock().unwrap().as_ref()).map_or(Timestamp(0), Clock::now);
        self.add(thread, author, false, body, now)
    }

    /// Gives `login` write access to the repo
    pub(crate) fn grant_write(&self, login: &str) {
        self.threads
            .lock()
            .unwrap()
            .writers
            .insert(login.to_owned());
    }

    /// `author` writes `body` on `thread` at `created`, and the comment's id
    pub(crate) fn write(&self, thread: u64, author: &str, body: &str, created: Timestamp) -> u64 {
        self.add(thread, author, false, body, created)
    }

    /// A bot writes `body` on `thread` at `created`
    pub(crate) fn bot_writes(&self, thread: u64, author: &str, body: &str, created: Timestamp) {
        self.add(thread, author, true, body, created);
    }

    fn add(&self, thread: u64, author: &str, bot: bool, body: &str, created: Timestamp) -> u64 {
        let mut threads = self.threads.lock().unwrap();
        let id = FIRST + threads.comments.len() as u64;
        threads.comments.push(ThreadComment {
            id,
            thread,
            author: author.to_owned(),
            bot,
            body: body.to_owned(),
            created,
        });
        id
    }

    /// `login` gives comment `comment` a thumbs up, or a thumbs down
    pub(crate) fn thumbs(&self, comment: u64, login: &str, up: bool) {
        let mut threads = self.threads.lock().unwrap();
        let login = login.to_owned();
        (threads.marks.entry(comment).or_default().thumbs).push(Thumb { login, up });
    }

    /// `editor` edits comment `comment` to say `body`
    pub(crate) fn edits(&self, comment: u64, editor: &str, body: &str) {
        let mut threads = self.threads.lock().unwrap();
        let marks = threads.marks.entry(comment).or_default();
        marks.body = body.to_owned();
        marks.editors.push(editor.to_owned());
    }

    /// Makes reading what was done to comment `comment` fail
    pub(crate) fn cannot_read_marks_of(&self, comment: u64) {
        self.threads.lock().unwrap().unreadable.insert(comment);
    }

    /// What was done to comment `comment`, as the forge tells it, or none
    /// when it cannot be read
    pub(super) fn marks_of(&self, comment: u64) -> Option<CommentMarks> {
        let threads = self.threads.lock().unwrap();
        if threads.unreadable.contains(&comment) {
            return None;
        }
        let mut marks = threads.marks.get(&comment).cloned().unwrap_or_default();
        // The App wrote it, and GraphQL names an App's login without `[bot]`.
        marks.author = "kelpie-octocat".to_owned();
        Some(marks)
    }

    /// The `since` of each read of the repo's comments, in order
    pub(crate) fn reads(&self) -> Vec<Timestamp> {
        self.threads.lock().unwrap().reads.clone()
    }

    /// The login of each ask whether it may write, in order
    pub(crate) fn asked(&self) -> Vec<String> {
        self.threads.lock().unwrap().asked.clone()
    }

    /// The comments written since `since`, oldest first, noting the read
    pub(super) fn read(&self, since: Timestamp) -> Vec<ThreadComment> {
        let mut threads = self.threads.lock().unwrap();
        threads.reads.push(since);
        let mut read: Vec<_> = (threads.comments.iter())
            .filter(|c| c.created >= since)
            .cloned()
            .collect();
        read.sort_by_key(|c| c.id);
        read
    }

    /// Whether `login` may write, noting the ask
    pub(super) fn may_write(&self, login: &str) -> bool {
        let mut threads = self.threads.lock().unwrap();
        threads.asked.push(login.to_owned());
        threads.writers.contains(login)
    }
}
