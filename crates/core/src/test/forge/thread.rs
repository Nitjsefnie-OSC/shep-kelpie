//! The comments on the fake forge's threads, and who may write to its repo

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::VIEWER;
use crate::ports::{Clock, CommentMarks, Remark, Standing, ThreadComment, Thumb, Timestamp};
use crate::test::FakeClock;

/// The first id a comment on the fake forge gets
const FIRST: u64 = 5000;

/// The first id a comment in a review thread gets, numbered apart from the
/// others as the forge numbers them
const FIRST_REVIEW: u64 = 8000;

#[derive(Debug, Default)]
struct Threads {
    comments: Vec<ThreadComment>,
    // The comments in review threads, by the pull request they are on
    reviews: Vec<ThreadComment>,
    // The pull request and `since` of each read of review threads
    review_reads: Vec<(u64, Timestamp)>,
    writers: HashSet<String>,
    // The `since` of each read of the repo's comments
    reads: Vec<Timestamp>,
    // The login of each ask whether it may write
    asked: Vec<String>,
    // What people did to the comments kelpie posted, by comment id
    marks: HashMap<u64, CommentMarks>,
    // The comments whose marks cannot be read
    unreadable: HashSet<u64>,
    // The threads whose comments cannot be listed
    unlisted: HashSet<u64>,
}

/// What people wrote on the repo's issues and pull requests
#[derive(Debug, Clone, Default)]
pub struct FakeThreads {
    threads: Arc<Mutex<Threads>>,
    // The clock the comments kelpie posts as its own login are written by
    clock: Arc<Mutex<Option<FakeClock>>>,
}

impl FakeThreads {
    /// Has the comments kelpie posts as its own login written by `clock`
    pub fn use_clock(&self, clock: FakeClock) {
        *self.clock.lock().unwrap() = Some(clock);
    }

    /// `author` writes `body` on `thread` now, by the clock given, and the comment's id
    pub fn write_now(&self, thread: u64, author: &str, body: &str) -> u64 {
        let now = (self.clock.lock().unwrap().as_ref()).map_or(Timestamp(0), Clock::now);
        self.add(thread, author, false, body, now)
    }

    /// Gives `login` write access to the repo
    pub fn grant_write(&self, login: &str) {
        self.threads
            .lock()
            .unwrap()
            .writers
            .insert(login.to_owned());
    }

    /// `author` writes `body` on `thread` at `created`, and the comment's id
    pub fn write(&self, thread: u64, author: &str, body: &str, created: Timestamp) -> u64 {
        self.add(thread, author, false, body, created)
    }

    /// A bot writes `body` on `thread` at `created`
    pub fn bot_writes(&self, thread: u64, author: &str, body: &str, created: Timestamp) {
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

    /// `author` writes `body` in a review thread of pull request `number` at
    /// `created`, and the comment's id
    pub fn writes_in_review(
        &self,
        number: u64,
        author: &str,
        body: &str,
        created: Timestamp,
    ) -> u64 {
        self.add_review(number, author, false, body, created)
    }

    /// A bot writes `body` in a review thread of pull request `number` at `created`
    pub fn bot_writes_in_review(
        &self,
        number: u64,
        author: &str,
        body: &str,
        created: Timestamp,
    ) -> u64 {
        self.add_review(number, author, true, body, created)
    }

    fn add_review(
        &self,
        thread: u64,
        author: &str,
        bot: bool,
        body: &str,
        created: Timestamp,
    ) -> u64 {
        let mut threads = self.threads.lock().unwrap();
        let id = FIRST_REVIEW + threads.reviews.len() as u64;
        threads.reviews.push(ThreadComment {
            id,
            thread,
            author: author.to_owned(),
            bot,
            body: body.to_owned(),
            created,
        });
        id
    }

    /// The pull request and `since` of each read of review threads, in order
    pub fn review_reads(&self) -> Vec<(u64, Timestamp)> {
        self.threads.lock().unwrap().review_reads.clone()
    }

    /// The comments in review threads of pull request `number` written since
    /// `since`, oldest first, noting the read
    pub fn read_reviews(&self, number: u64, since: Timestamp) -> Vec<ThreadComment> {
        let mut threads = self.threads.lock().unwrap();
        threads.review_reads.push((number, since));
        let mut read: Vec<_> = (threads.reviews.iter())
            .filter(|c| c.thread == number && c.created >= since)
            .cloned()
            .collect();
        read.sort_by_key(|c| (c.created, c.id));
        read
    }

    /// `login` gives comment `comment` a thumbs up, or a thumbs down
    pub fn thumbs(&self, comment: u64, login: &str, up: bool) {
        let mut threads = self.threads.lock().unwrap();
        let login = login.to_owned();
        (threads.marks.entry(comment).or_default().thumbs).push(Thumb { login, up });
    }

    /// `editor` edits comment `comment` to say `body`
    pub fn edits(&self, comment: u64, editor: &str, body: &str) {
        let mut threads = self.threads.lock().unwrap();
        let marks = threads.marks.entry(comment).or_default();
        marks.body = body.to_owned();
        marks.editors.push(editor.to_owned());
    }

    /// Makes reading what was done to comment `comment` fail
    pub fn cannot_read_marks_of(&self, comment: u64) {
        self.threads.lock().unwrap().unreadable.insert(comment);
    }

    /// Lets comment `comment`'s marks be read again
    pub fn can_read_marks_of(&self, comment: u64) {
        self.threads.lock().unwrap().unreadable.remove(&comment);
    }

    /// What was done to comment `comment`, as the forge tells it, or none
    /// when it cannot be read
    pub fn marks_of(&self, comment: u64) -> Option<CommentMarks> {
        let threads = self.threads.lock().unwrap();
        if threads.unreadable.contains(&comment) {
            return None;
        }
        let mut marks = threads.marks.get(&comment).cloned().unwrap_or_default();
        // An unedited comment says what was written.
        if marks.body.is_empty() {
            let written = threads.comments.iter().find(|c| c.id == comment);
            marks.body = written.map_or_else(String::new, |c| c.body.clone());
        }
        // The App wrote it, and GraphQL names an App's login without `[bot]`.
        marks.author = "kelpie-octocat".to_owned();
        Some(marks)
    }

    /// The `since` of each read of the repo's comments, in order
    pub fn reads(&self) -> Vec<Timestamp> {
        self.threads.lock().unwrap().reads.clone()
    }

    /// The login of each ask whether it may write, in order
    pub fn asked(&self) -> Vec<String> {
        self.threads.lock().unwrap().asked.clone()
    }

    /// The comments written since `since`, oldest first, noting the read
    pub fn read(&self, since: Timestamp) -> Vec<ThreadComment> {
        let mut threads = self.threads.lock().unwrap();
        threads.reads.push(since);
        let mut read: Vec<_> = (threads.comments.iter())
            .filter(|c| c.created >= since)
            .cloned()
            .collect();
        read.sort_by_key(|c| c.id);
        read
    }

    /// Every comment on issue `number`, oldest first
    pub fn on(&self, number: u64) -> Vec<ThreadComment> {
        let threads = self.threads.lock().unwrap();
        (threads.comments.iter())
            .filter(|c| c.thread == number)
            .cloned()
            .collect()
    }

    /// The comments on conversation `number`, oldest first, as the forge's
    /// GraphQL tells them: kelpie's own login and the logins given write
    /// access are writers
    pub fn remarks(&self, number: u64) -> Vec<Remark> {
        let threads = self.threads.lock().unwrap();
        (threads.comments.iter())
            .filter(|c| c.thread == number)
            .map(|c| Remark {
                id: c.id,
                author: c.author.clone(),
                standing: match (
                    c.bot,
                    threads.writers.contains(&c.author) || c.author == VIEWER,
                ) {
                    (true, _) => Standing::Bot,
                    (false, true) => Standing::Writer,
                    (false, false) => Standing::Other,
                },
                body: c.body.clone(),
            })
            .collect()
    }

    /// Makes listing the comments on thread `number` fail, or work again
    pub fn set_unlisted(&self, number: u64, unlisted: bool) {
        let mut threads = self.threads.lock().unwrap();
        if unlisted {
            threads.unlisted.insert(number);
        } else {
            threads.unlisted.remove(&number);
        }
    }

    /// Whether listing the comments on thread `number` fails
    pub fn is_unlisted(&self, number: u64) -> bool {
        self.threads.lock().unwrap().unlisted.contains(&number)
    }

    /// Whether `login` may write, noting the ask
    pub fn may_write(&self, login: &str) -> bool {
        let mut threads = self.threads.lock().unwrap();
        threads.asked.push(login.to_owned());
        threads.writers.contains(login)
    }
}
