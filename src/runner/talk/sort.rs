//! Which of the comments on a pull request are requests of the worker

use std::collections::BTreeSet;

use super::COMMENT_REQUEST;
use crate::ports::{Remark, Standing, Talk};
use crate::work_item::Talked;

// How the replies kelpie posts on a thread begin. Under the maintainer's own
// login, `gh` posts them, and they are no one's request.
const OWN_REPLIES: [&str; 4] = ["Fixed.", "Not fixed:", "Declined:", "Not addressed:"];

/// What was written, to be sent to the worker as one numbered request
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Request {
    /// What the worker's reply names it by once answered: a review thread's
    /// id, or `comment:` and a conversation comment's id
    pub id: String,
    /// The file and line of a review thread
    pub place: Option<String>,
    /// Who wrote what, oldest first
    pub said: Vec<(String, String)>,
}

/// What a read found
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Sorted {
    pub requests: Vec<Request>,
    /// One line for each comment left alone because of who wrote it
    pub ignored: Vec<String>,
    /// The conversation comments handled, requests and ignored alike
    pub comments: Vec<u64>,
    /// Each review thread handled, and the latest comment of it
    pub threads: Vec<(String, u64)>,
}

/// What a read is sorted by
pub(super) struct Sieve<'a> {
    pub talked: &'a Talked,
    /// The ids of the comments kelpie posted itself
    pub own: &'a BTreeSet<u64>,
    /// Logins whose conversation comments are answers to a ruling, which the
    /// ruling's thread reads
    pub left: &'a [String],
    /// Whether what is already written is only noted: a pull request kelpie
    /// did not open has comments the worker has seen
    pub quiet: bool,
}

pub(super) fn sort(talk: &Talk, sieve: &Sieve<'_>) -> Sorted {
    let mut sorted = Sorted::default();
    for comment in &talk.comments {
        if sieve.talked.comments.contains(&comment.id) {
            continue;
        }
        if sieve.own.contains(&comment.id) {
            sorted.comments.push(comment.id);
            continue;
        }
        if comment.standing != Standing::Writer {
            sorted.ignored.push(ignored(comment, "the conversation"));
            sorted.comments.push(comment.id);
            continue;
        }
        let aimed_at_a_ruling = sieve
            .left
            .iter()
            .any(|login| login.eq_ignore_ascii_case(&comment.author));
        if aimed_at_a_ruling {
            continue;
        }
        sorted.comments.push(comment.id);
        if !sieve.quiet {
            sorted.requests.push(Request {
                id: format!("{COMMENT_REQUEST}{}", comment.id),
                place: None,
                said: vec![(comment.author.clone(), comment.body.clone())],
            });
        }
    }
    for thread in talk.threads.iter().filter(|t| !t.resolved) {
        let seen = sieve.talked.threads.get(&thread.id).copied().unwrap_or(0);
        let Some(latest) = thread.remarks.iter().map(|r| r.id).max().filter(|l| *l > seen) else {
            continue;
        };
        sorted.threads.push((thread.id.clone(), latest));
        let place = match thread.line {
            Some(line) => format!("{}:{line}", thread.path),
            None => thread.path.clone(),
        };
        let mut asked = false;
        let mut said = Vec::new();
        for remark in &thread.remarks {
            let fresh = remark.id > seen;
            match remark.standing {
                Standing::Writer if !is_own_reply(remark) => {
                    asked |= fresh;
                    said.push((remark.author.clone(), remark.body.clone()));
                }
                Standing::Writer => {}
                _ if fresh => sorted.ignored.push(ignored(remark, &place)),
                _ => {}
            }
        }
        if asked && !sieve.quiet {
            sorted.requests.push(Request {
                id: thread.id.clone(),
                place: Some(place),
                said,
            });
        }
    }
    sorted
}

fn is_own_reply(remark: &Remark) -> bool {
    OWN_REPLIES.iter().any(|start| remark.body.starts_with(start))
}

fn ignored(remark: &Remark, on: &str) -> String {
    let who = match remark.standing {
        Standing::Bot => "an App or a bot",
        _ => "someone without write access",
    };
    format!(
        "ignored comment {} by {} on {on}: {who} is not trusted to ask for changes",
        remark.id, remark.author
    )
}
