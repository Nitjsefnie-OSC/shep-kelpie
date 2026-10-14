//! Reading through the App's token where the App covers the repo
//!
//! The App's installation has a rate budget of its own, so the reads it can
//! make leave the maintainer's login for the writes and for what the App
//! cannot read. A read goes to the App when [`AppForges`] says it covers the
//! repo and its token holds the permissions the read needs. Otherwise it goes
//! to the login, and when the App was to read it and could not, that is said
//! once with the permission that was missing.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use super::{RateHeld, fallback_from, said_until, time_of, used_up};
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::forge_ledger::{Ended, Identity, Kind};
use crate::ports::{Forge, ForgeError, Issue, PullRequest, Reviewed, Timestamp};
use crate::review_bot::{Activity, Login};
use crate::settings::ForgeSlug;

/// The reads that run on the App's token, as the App
pub trait AppReads: Send + Sync + fmt::Debug {
    /// As [`Forge::issue`]
    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError>;

    /// As [`Forge::ready_issues`]
    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError>;

    /// As [`Forge::open_pull_requests`]
    fn open_pull_requests(&self, repo: &ForgeSlug) -> Result<Vec<OpenPullRequest>, ForgeError>;

    /// As [`Forge::pull_request`]
    fn pull_request(&self, repo: &ForgeSlug, number: u64) -> Result<PullRequest, ForgeError>;

    /// As [`Forge::reviewed`]
    fn reviewed(&self, repo: &ForgeSlug, number: u64) -> Result<Reviewed, ForgeError>;

    /// As [`Forge::review_bot`]
    fn review_bot(
        &self,
        repo: &ForgeSlug,
        number: u64,
        login: Login<'_>,
    ) -> Result<Activity, ForgeError>;
}

/// Who reads a repo
#[derive(Debug, Clone)]
pub enum Reading {
    /// The App covers the repo: its reads, and the permissions its token holds
    App {
        /// The reads, as the App
        reads: Arc<dyn AppReads>,
        /// The names of the permissions the token holds
        holds: BTreeSet<String>,
    },
    /// The App does not cover the repo, so the login reads it
    Login,
}

/// Says who reads a repo
pub trait AppForges: Send + Sync {
    /// The App's reads of `repo` where it covers it
    fn reading(&self, repo: &ForgeSlug) -> Reading;
}

/// The reads the App makes, each with the permissions it needs
#[derive(Debug, Clone, Copy)]
pub(super) enum Read {
    Issue,
    ReadyIssues,
    OpenPullRequests,
    PullRequest,
    Reviewed,
    ReviewBot,
}

impl Read {
    fn kind(self) -> Kind {
        match self {
            Self::Issue => Kind::Issue,
            Self::ReadyIssues => Kind::ReadyIssues,
            Self::OpenPullRequests => Kind::OpenPullRequests,
            Self::PullRequest => Kind::PullRequest,
            Self::Reviewed => Kind::Reviewed,
            Self::ReviewBot => Kind::ReviewBot,
        }
    }

    /// The permissions, each to read, the read's calls need
    pub(super) fn needs(self) -> &'static [&'static str] {
        match self {
            Self::Issue | Self::ReadyIssues => &["issues"],
            Self::OpenPullRequests | Self::Reviewed => &["pull_requests"],
            Self::PullRequest => &["pull_requests", "checks", "statuses"],
            Self::ReviewBot => &["pull_requests", "issues", "statuses"],
        }
    }
}

/// Who reads, and what has gone wrong with the App reading so far
pub(super) struct AppReader {
    forges: Arc<dyn AppForges>,
    // Until when the App's rate limit is used up, so the login reads meanwhile
    held: Mutex<Option<Timestamp>>,
    // Each permission a repo's token was said to lack, with the key of the
    // warning that named it, so each is said once
    told: Mutex<BTreeMap<String, String>>,
}

impl AppReader {
    pub(super) fn new(forges: Arc<dyn AppForges>) -> Self {
        Self {
            forges,
            held: Mutex::new(None),
            told: Mutex::new(BTreeMap::new()),
        }
    }
}

// What gh prints when the App's token lacks the permission a call needs
const NOT_ACCESSIBLE: &str = "resource not accessible by integration";

// The key of the warning about one permission a repo's token lacks.
fn warning_key(cause: Cause, repo: &ForgeSlug, permission: &str) -> String {
    let cause = match cause {
        Cause::Lacks => "app-lacks",
        Cause::Refused => "app-refused",
    };
    format!("{cause} {permission} on {}", repo.as_str())
}

// Why the App's reads fall to the login: the token is known to lack a
// permission, which a grant ends, or a read was refused for want of one.
#[derive(Clone, Copy)]
enum Cause {
    Lacks,
    Refused,
}

fn lacks_permission(error: &ForgeError) -> bool {
    matches!(error, ForgeError::Failed(said) if said.to_lowercase().contains(NOT_ACCESSIBLE))
}

impl RateHeld {
    /// Reads through the App where `forges` says it covers the repo, and
    /// through the login everywhere else
    #[must_use]
    pub fn with_app_reads(mut self, forges: Arc<dyn AppForges>) -> Self {
        self.app = Some(AppReader::new(forges));
        self
    }

    // `read` of `repo`, as the App where it covers the repo and its token
    // holds what the read needs, and as the login otherwise.
    pub(super) fn read<T>(
        &self,
        repo: &ForgeSlug,
        read: Read,
        app: impl FnOnce(&dyn AppReads) -> Result<T, ForgeError>,
        login: impl FnOnce(&dyn Forge) -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        if let Some(reads) = self.reads_as_app(repo, read) {
            match self.call_as_app(read, || app(reads.as_ref())) {
                Err(error) if lacks_permission(&error) => self.told_lacking(repo, read, &error),
                Err(error) if used_up(&error) => {}
                result => return result,
            }
        }
        self.call(read.kind(), login)
    }

    // The App's reads, when it covers the repo, its limit is not used up and
    // its token holds what `read` needs.
    fn reads_as_app(&self, repo: &ForgeSlug, read: Read) -> Option<Arc<dyn AppReads>> {
        let reader = self.app.as_ref()?;
        let now = self.clock.now();
        let held = reader.held.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_some_and(|until| until > now) {
            return None;
        }
        drop(held);
        let Reading::App { reads, holds } = reader.forges.reading(repo) else {
            return None;
        };
        let lacking: Vec<&str> = read
            .needs()
            .iter()
            .copied()
            .filter(|need| !holds.contains(*need))
            .collect();
        if lacking.is_empty() {
            self.holds_again(repo, read.needs());
            return Some(reads);
        }
        self.tell_once(
            Cause::Lacks,
            repo,
            &lacking,
            format!(
                "the App's token for {} lacks the {} permission to read, which the {} read \
                 needs, so the login reads it: add it in the App's settings, then accept it on \
                 the installation",
                repo.as_str(),
                lacking.join(" and "),
                read.kind().name(),
            ),
        );
        None
    }

    fn told_lacking(&self, repo: &ForgeSlug, read: Read, error: &ForgeError) {
        self.tell_once(
            Cause::Refused,
            repo,
            read.needs(),
            format!(
                "the App was refused the {} read of {} ({error}); it needs the {} permission to \
                 read, so the login reads it",
                read.kind().name(),
                repo.as_str(),
                read.needs().join(" and "),
            ),
        );
    }

    // Told once for each permission a repo's reads lack, whichever read met
    // it: a read that lacks only permissions told already says nothing.
    fn tell_once(&self, cause: Cause, repo: &ForgeSlug, permissions: &[&str], line: String) {
        let Some(reader) = &self.app else { return };
        let mut told = reader.told.lock().unwrap_or_else(PoisonError::into_inner);
        let new: Vec<&str> = (permissions.iter().copied())
            .filter(|permission| !told.contains_key(&warning_key(cause, repo, permission)))
            .collect();
        if new.is_empty() {
            return;
        }
        let key = warning_key(cause, repo, &new.join(" and "));
        for permission in &new {
            told.insert(warning_key(cause, repo, permission), key.clone());
        }
        drop(told);
        match &self.standing {
            Some(standing) => standing.stands(&key, line),
            None => (self.say)(line),
        }
    }

    // The token holds what a read needs: a warning stands no longer once
    // every permission it named is held.
    fn holds_again(&self, repo: &ForgeSlug, permissions: &[&str]) {
        let Some(reader) = &self.app else { return };
        let mut told = reader.told.lock().unwrap_or_else(PoisonError::into_inner);
        let ended: Vec<String> = (permissions.iter())
            .filter_map(|permission| told.remove(&warning_key(Cause::Lacks, repo, permission)))
            .collect();
        let over: Vec<String> = ended
            .into_iter()
            .filter(|key| !told.values().any(|stands| stands == key))
            .collect();
        drop(told);
        if let Some(standing) = &self.standing {
            for key in over {
                standing.ends(&key);
            }
        }
    }

    // A call the App makes. It spends the App's budget and not the login's,
    // so it takes no turn in the login's pace, and an answer that the App's
    // limit is used up sends its reads to the login until the limit resets.
    fn call_as_app<T>(
        &self,
        read: Read,
        call: impl FnOnce() -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        let now = self.clock.now();
        let started = std::time::Instant::now();
        let result = call();
        let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ended = match &result {
            Ok(_) => Ended::Ok,
            Err(error) if used_up(error) => Ended::UsedUp,
            Err(_) => Ended::Failed,
        };
        self.record_by(Identity::App, self.clock.now(), read.kind(), millis, ended);
        if let Err(error) = &result
            && ended == Ended::UsedUp
            && let Some(reader) = &self.app
        {
            let until = said_until(error, now).unwrap_or(fallback_from(now));
            *reader.held.lock().unwrap_or_else(PoisonError::into_inner) = Some(until);
            (self.say)(format!(
                "the App's rate limit is used up ({error}), so the login makes its reads until {}",
                time_of(until)
            ));
        }
        result
    }
}

#[cfg(test)]
mod tests;
