//! The comments on the fake forge's threads, and who may write to its repo

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::ports::{Clock, ThreadComment, Timestamp};
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
