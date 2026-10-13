//! The fake forge's reads of pull requests, and the count of each kind

use std::sync::atomic::AtomicUsize;

use super::FakeForge;
use crate::board::{OpenPullRequest, ReviewMarks};
use crate::ports::{Checks, ForgeError, PullRequest, PullRequestState};

/// How many times each kind of read of pull requests was made
#[derive(Debug, Default)]
pub(crate) struct PullRequestReads {
    /// The listing of every open pull request
    pub(crate) listed: AtomicUsize,
    /// One pull request's state, head and checks
    pub(crate) viewed: AtomicUsize,
    /// One pull request's review
    pub(crate) reviewed: AtomicUsize,
}

impl FakeForge {
    /// The reads of pull requests made so far
    pub(crate) fn pull_request_reads(&self) -> &PullRequestReads {
        &self.pull_request_reads
    }

    pub(super) fn count_review(&self, number: u64) {
        let mut counts = self.review_counts.lock().unwrap();
        *counts.entry(number).or_default() += 1;
    }

    // Pull request `number` as the forge shows it now, with no call counted
    pub(super) fn read(&self, number: u64) -> Result<PullRequest, ForgeError> {
        let pr = self.opened(number)?;
        let lagging = self.lagging.lock().unwrap().get(&number).cloned();
        let head = match lagging {
            Some(head) => head,
            None => self
                .head_of(&pr.branch)
                .ok_or_else(|| ForgeError::Failed(format!("no branch {} on origin", pr.branch)))?,
        };
        let checks = self.checks.lock().unwrap().get(&head).cloned();
        Ok(PullRequest {
            state: pr.state,
            draft: pr.draft || self.lagging_drafts.lock().unwrap().contains(&number),
            checks: checks.unwrap_or(Checks::Pending),
            head,
            labels: self.coderabbit.labels(number),
        })
    }

    // Every open pull request as `read` shows it, one whose branch is not on
    // origin with no head and no checks, with no call counted
    pub(super) fn list_open(&self) -> Result<Vec<OpenPullRequest>, ForgeError> {
        let open = self.open.lock().unwrap().clone();
        let mut listed = Vec::new();
        for pr in open {
            let fake = self.opened(pr.number)?;
            if fake.state != PullRequestState::Open {
                continue;
            }
            let view = self.read(pr.number).ok();
            let reviews = self.review_counts.lock().unwrap().get(&pr.number).copied();
            listed.push(OpenPullRequest {
                labels: self.coderabbit.labels(pr.number),
                draft: view.as_ref().map_or(fake.draft, |v| v.draft),
                commit: view.as_ref().map(|v| v.head.clone()).unwrap_or_default(),
                checks: view.map_or(Checks::None, |v| v.checks),
                from_fork: fake.from_fork,
                author: fake.author,
                marks: ReviewMarks {
                    reviews: reviews.unwrap_or_default(),
                    threads: 0,
                },
                ..pr
            });
        }
        Ok(listed)
    }
}
