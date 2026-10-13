//! Holding every forge call while the forge's rate limit is used up
//!
//! A runner's forge goes through [`RateHeld`]. A call the forge answers
//! with its rate limit used up holds every later call until the limit
//! resets, read once from the forge, or for [`FALLBACK`] when the forge
//! cannot say. A held call fails at once with [`ForgeError::Held`], and
//! each hold is said once, as it begins, through [`RateHeld`]'s [`Say`].

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use super::{
    Clock, CommentMarks, FolderEntry, Forge, ForgeError, Issue, NewLabel, OpenIssue, PullRequest,
    QueueStanding, Reviewed, ThreadComment, Timestamp, Visibility,
};
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::forge_ledger::{Ended, Kind, Ledger};
use crate::review_bot::{Activity, Login};
use crate::settings::ForgeSlug;

/// How long a hold lasts when the forge cannot say when its limit resets
const FALLBACK: u64 = 10 * 60;

/// What gh says when a limit is used up, lowercased: the REST and GraphQL
/// primary limits, the secondary limit, and an HTTP 429 answer
const USED_UP: [&str; 4] = [
    "api rate limit exceeded",
    "api rate limit already exceeded",
    "secondary rate limit",
    "http 429",
];

/// Whether `error` is the forge saying its rate limit is used up
fn used_up(error: &ForgeError) -> bool {
    match error {
        ForgeError::Limited { .. } => true,
        ForgeError::Failed(stderr) => says_used_up(stderr),
        _ => false,
    }
}

/// Whether a failed gh call's `stderr` says a rate limit is used up
pub fn says_used_up(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    USED_UP.iter().any(|said| stderr.contains(said))
}

/// `at` as a UTC time, as `2026-10-10T05:31:54Z`
pub(super) fn time_of(at: Timestamp) -> String {
    i64::try_from(at.0)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map_or_else(|| at.0.to_string(), |at| at.to_string())
}

/// Until when the forge is held, shared by [`RateHeld`] and its runner
#[derive(Debug, Clone, Default)]
pub struct ForgeHold(Arc<Mutex<Option<Timestamp>>>);

impl ForgeHold {
    /// Until when every forge call is held, while that is after `now`
    pub fn until(&self, now: Timestamp) -> Option<Timestamp> {
        let mut until = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if until.is_some_and(|until| until <= now) {
            *until = None;
        }
        *until
    }

    fn hold(&self, until: Timestamp) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(until);
    }
}

/// Where [`RateHeld`] says a hold began, once for each
pub type Say = Box<dyn Fn(String) + Send + Sync>;

/// A forge whose calls are held while its rate limit is used up
pub struct RateHeld {
    forge: Arc<dyn Forge>,
    clock: Arc<dyn Clock>,
    hold: ForgeHold,
    say: Say,
    ledger: Option<Arc<Ledger>>,
}

impl RateHeld {
    /// `forge`, holding its calls in `hold` by `clock`'s time, and telling
    /// `say` as each hold begins
    pub fn new(forge: Arc<dyn Forge>, clock: Arc<dyn Clock>, hold: ForgeHold, say: Say) -> Self {
        Self {
            forge,
            clock,
            hold,
            say,
            ledger: None,
        }
    }

    /// Records each call in `ledger`, held ones included
    #[must_use]
    pub fn with_ledger(mut self, ledger: Arc<Ledger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    fn call<T>(
        &self,
        kind: Kind,
        call: impl FnOnce(&dyn Forge) -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        let now = self.clock.now();
        if let Some(until) = self.hold.until(now) {
            self.record(now, kind, 0, Ended::Held);
            return Err(ForgeError::Held(until));
        }
        let started = Instant::now();
        let result = call(self.forge.as_ref());
        let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ended = match &result {
            Ok(_) => Ended::Ok,
            Err(error) if used_up(error) => Ended::UsedUp,
            Err(_) => Ended::Failed,
        };
        self.record(self.clock.now(), kind, millis, ended);
        if let Err(error) = &result
            && ended == Ended::UsedUp
        {
            let until = self.held_until(error, now);
            self.hold.hold(until);
            (self.say)(format!(
                "the forge's rate limit is used up ({error}), so kelpie makes no forge call \
                 until {}",
                time_of(until)
            ));
        }
        result
    }

    // What the answer itself says of when to ask again comes first, then the
    // forge's reset, then the fallback.
    fn held_until(&self, error: &ForgeError, now: Timestamp) -> Timestamp {
        let said = match error {
            ForgeError::Limited { cooldown, .. } => cooldown.until(now),
            _ => None,
        };
        said.or_else(|| match self.reset() {
            Ok(Some(reset)) if reset > now => Some(reset),
            _ => None,
        })
        .unwrap_or(Timestamp(now.0.saturating_add(FALLBACK)))
    }

    // The reset is a call to the forge too, so it is recorded like the rest.
    fn reset(&self) -> Result<Option<Timestamp>, ForgeError> {
        let started = Instant::now();
        let result = self.forge.rate_limit_reset();
        let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ended = if result.is_ok() {
            Ended::Ok
        } else {
            Ended::Failed
        };
        self.record(self.clock.now(), Kind::RateLimitReset, millis, ended);
        result
    }

    fn record(&self, at: Timestamp, kind: Kind, millis: u64, ended: Ended) {
        if let Some(ledger) = &self.ledger {
            ledger.record(at, kind, millis, ended);
        }
    }
}

impl fmt::Debug for RateHeld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RateHeld")
            .field("hold", &self.hold)
            .finish_non_exhaustive()
    }
}

impl Forge for RateHeld {
    fn visibility(&self, repo: &ForgeSlug) -> Result<Visibility, ForgeError> {
        self.call(Kind::Visibility, |f| f.visibility(repo))
    }

    fn default_branch(&self, repo: &ForgeSlug) -> Result<String, ForgeError> {
        self.call(Kind::DefaultBranch, |f| f.default_branch(repo))
    }

    fn folder(&self, repo: &ForgeSlug, path: &str) -> Result<Option<Vec<FolderEntry>>, ForgeError> {
        self.call(Kind::Folder, |f| f.folder(repo, path))
    }

    fn file_text(&self, repo: &ForgeSlug, path: &str) -> Result<String, ForgeError> {
        self.call(Kind::FileText, |f| f.file_text(repo, path))
    }

    fn repo_labels(&self, repo: &ForgeSlug) -> Result<Vec<String>, ForgeError> {
        self.call(Kind::RepoLabels, |f| f.repo_labels(repo))
    }

    fn create_label(&self, repo: &ForgeSlug, label: &NewLabel) -> Result<(), ForgeError> {
        self.call(Kind::CreateLabel, |f| f.create_label(repo, label))
    }

    fn can_push(&self, repo: &ForgeSlug) -> Result<bool, ForgeError> {
        self.call(Kind::CanPush, |f| f.can_push(repo))
    }

    fn review_bot_seen(&self, repo: &ForgeSlug, login: Login<'_>) -> Result<bool, ForgeError> {
        self.call(Kind::ReviewBotSeen, |f| f.review_bot_seen(repo, login))
    }

    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError> {
        self.call(Kind::Issue, |f| f.issue(repo, number))
    }

    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        self.call(Kind::ReadyIssues, |f| f.ready_issues(repo))
    }

    fn open_pull_requests(&self, repo: &ForgeSlug) -> Result<Vec<OpenPullRequest>, ForgeError> {
        self.call(Kind::OpenPullRequests, |f| f.open_pull_requests(repo))
    }

    fn pull_request(&self, repo: &ForgeSlug, number: u64) -> Result<PullRequest, ForgeError> {
        self.call(Kind::PullRequest, |f| f.pull_request(repo, number))
    }

    fn reviewed(&self, repo: &ForgeSlug, number: u64) -> Result<Reviewed, ForgeError> {
        self.call(Kind::Reviewed, |f| f.reviewed(repo, number))
    }

    fn viewer(&self) -> Result<String, ForgeError> {
        self.call(Kind::Viewer, |f| f.viewer())
    }

    fn comments_since(
        &self,
        repo: &ForgeSlug,
        since: Timestamp,
    ) -> Result<Vec<ThreadComment>, ForgeError> {
        self.call(Kind::CommentsSince, |f| f.comments_since(repo, since))
    }

    fn can_write(&self, repo: &ForgeSlug, login: &str) -> Result<bool, ForgeError> {
        self.call(Kind::CanWrite, |f| f.can_write(repo, login))
    }

    fn comment_marks(&self, repo: &ForgeSlug, comment: u64) -> Result<CommentMarks, ForgeError> {
        self.call(Kind::CommentMarks, |f| f.comment_marks(repo, comment))
    }

    fn comment(&self, repo: &ForgeSlug, number: u64, body: &str) -> Result<u64, ForgeError> {
        self.call(Kind::Comment, |f| f.comment(repo, number, body))
    }

    fn open_issues(&self, repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
        self.call(Kind::OpenIssues, |f| f.open_issues(repo))
    }

    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        self.call(Kind::CreateIssue, |f| {
            f.create_issue(repo, title, body, labels)
        })
    }

    fn close_issue(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        self.call(Kind::CloseIssue, |f| f.close_issue(repo, number))
    }

    fn create_pull_request(
        &self,
        repo: &ForgeSlug,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<u64, ForgeError> {
        self.call(Kind::CreatePullRequest, |f| {
            f.create_pull_request(repo, head, base, title, body)
        })
    }

    fn edit_pull_request(
        &self,
        repo: &ForgeSlug,
        number: u64,
        title: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        self.call(Kind::EditPullRequest, |f| {
            f.edit_pull_request(repo, number, title, body)
        })
    }

    fn failed_run_logs(
        &self,
        repo: &ForgeSlug,
        head: &str,
        merge_group: bool,
    ) -> Result<String, ForgeError> {
        self.call(Kind::FailedRunLogs, |f| {
            f.failed_run_logs(repo, head, merge_group)
        })
    }

    fn mark_ready(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        self.call(Kind::MarkReady, |f| f.mark_ready(repo, number))
    }

    fn set_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        self.call(Kind::SetLabel, |f| f.set_label(repo, number, label, on))
    }

    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        self.call(Kind::SetIssueLabel, |f| {
            f.set_issue_label(repo, number, label, on)
        })
    }

    fn review_bot(
        &self,
        repo: &ForgeSlug,
        number: u64,
        login: Login<'_>,
    ) -> Result<Activity, ForgeError> {
        self.call(Kind::ReviewBot, |f| f.review_bot(repo, number, login))
    }

    fn resolve_thread(&self, repo: &ForgeSlug, thread: &str) -> Result<(), ForgeError> {
        self.call(Kind::ResolveThread, |f| f.resolve_thread(repo, thread))
    }

    fn reply_to_thread(
        &self,
        repo: &ForgeSlug,
        thread: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        self.call(Kind::ReplyToThread, |f| {
            f.reply_to_thread(repo, thread, body)
        })
    }

    fn merge(&self, repo: &ForgeSlug, number: u64, head: &str) -> Result<(), ForgeError> {
        self.call(Kind::Merge, |f| f.merge(repo, number, head))
    }

    fn merge_queue(&self, repo: &ForgeSlug, number: u64) -> Result<QueueStanding, ForgeError> {
        self.call(Kind::MergeQueue, |f| f.merge_queue(repo, number))
    }

    fn disable_auto_merge(&self, repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        self.call(Kind::DisableAutoMerge, |f| {
            f.disable_auto_merge(repo, number)
        })
    }

    // Free of the limits, and asked only once a call finds them used up.
    fn rate_limit_reset(&self) -> Result<Option<Timestamp>, ForgeError> {
        self.reset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The first is from the live runner's log; the rest are GitHub's
    // documented messages, as gh prints a failed call.
    #[test]
    fn each_of_ghs_used_up_answers_is_a_used_up_limit() {
        let said = [
            "GraphQL: API rate limit already exceeded for user ID 1.",
            "gh: API rate limit exceeded for user ID 1. (HTTP 403)",
            "gh: You have exceeded a secondary rate limit. Please wait a few minutes before you \
             try again. (HTTP 403)",
            "gh: Too Many Requests (HTTP 429)",
        ];
        for stderr in said {
            assert!(used_up(&ForgeError::Failed(stderr.into())), "{stderr}");
        }
    }

    #[test]
    fn any_other_failure_is_not() {
        let other = [
            ForgeError::Failed("gh: Not Found (HTTP 404)".into()),
            ForgeError::Failed("error connecting to api.github.com".into()),
            ForgeError::Unreadable("API rate limit exceeded".into()),
        ];
        for error in other {
            assert!(!used_up(&error), "{error}");
        }
    }

    mod cooldown {
        use super::*;
        use crate::ports::Cooldown;
        use crate::test::{FakeClock, FakeForge};

        fn repo() -> ForgeSlug {
            ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
        }

        fn held_on(forge: &FakeForge, clock: &FakeClock) -> RateHeld {
            RateHeld::new(
                Arc::new(forge.clone()),
                Arc::new(clock.clone()),
                ForgeHold::default(),
                Box::new(|_| {}),
            )
        }

        #[test]
        fn a_retry_after_holds_for_that_long_and_not_for_the_reset_or_the_fallback() {
            let home = tempfile::tempdir().unwrap();
            let forge = FakeForge::new(home.path().join("origin"));
            let clock = FakeClock::at(1_000);
            let held = held_on(&forge, &clock);
            // The reset would hold for an hour, the fallback for ten minutes.
            forge.set_used_up(Some(Timestamp(4_600)));
            forge.set_limited(Cooldown {
                retry_after: Some(90),
                reset: None,
            });

            held.visibility(&repo()).unwrap_err();

            clock.advance(89);
            let held_for = held.viewer().unwrap_err();
            assert_eq!(held_for, ForgeError::Held(Timestamp(1_090)));
            forge.set_unlimited();
            forge.set_down(false);
            clock.advance(1);
            held.viewer().unwrap();
        }

        #[test]
        fn a_reset_in_the_answer_holds_until_then_without_asking_the_forge_for_it() {
            let home = tempfile::tempdir().unwrap();
            let forge = FakeForge::new(home.path().join("origin"));
            let clock = FakeClock::at(1_000);
            let held = held_on(&forge, &clock);
            forge.set_limited(Cooldown {
                retry_after: None,
                reset: Some(Timestamp(1_500)),
            });

            held.visibility(&repo()).unwrap_err();

            assert_eq!(forge.asked(), 1, "the reset was not asked for");
            assert_eq!(
                held.viewer().unwrap_err(),
                ForgeError::Held(Timestamp(1_500))
            );
        }

        #[test]
        fn an_answer_that_says_nothing_of_when_holds_for_the_fallback() {
            let home = tempfile::tempdir().unwrap();
            let forge = FakeForge::new(home.path().join("origin"));
            let clock = FakeClock::at(1_000);
            let held = held_on(&forge, &clock);
            forge.set_limited(Cooldown::default());

            held.visibility(&repo()).unwrap_err();

            assert_eq!(
                held.viewer().unwrap_err(),
                ForgeError::Held(Timestamp(1_000 + FALLBACK))
            );
        }
    }

    mod ledger {
        use super::*;
        use crate::forge_ledger::{self, Api, Line};
        use crate::test::{FakeClock, FakeForge};

        fn repo() -> ForgeSlug {
            ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
        }

        struct Rig {
            held: RateHeld,
            forge: FakeForge,
            clock: FakeClock,
            home: tempfile::TempDir,
        }

        impl Rig {
            fn new() -> Self {
                let home = tempfile::tempdir().unwrap();
                let forge = FakeForge::new(home.path().join("origin"));
                let clock = FakeClock::at(1_000);
                let held = RateHeld::new(
                    Arc::new(forge.clone()),
                    Arc::new(clock.clone()),
                    ForgeHold::default(),
                    Box::new(|_| {}),
                )
                .with_ledger(Arc::new(Ledger::in_home(home.path(), "koji")));
                Self {
                    held,
                    forge,
                    clock,
                    home,
                }
            }

            fn lines(&self) -> Vec<Line> {
                forge_ledger::read(&self.home.path().join(forge_ledger::FILE)).unwrap()
            }
        }

        #[test]
        fn each_call_adds_a_line_with_its_kind_and_how_it_ended() {
            let rig = Rig::new();
            rig.held.visibility(&repo()).unwrap();
            rig.clock.advance(30);
            rig.held.default_branch(&repo()).unwrap();
            rig.forge.set_down(true);
            rig.clock.advance(30);
            rig.held.repo_labels(&repo()).unwrap_err();

            let lines = rig.lines();
            let seen: Vec<_> = lines
                .iter()
                .map(|l| (l.at.0, l.project.as_str(), l.kind, l.api, l.ended))
                .collect();
            assert_eq!(
                seen,
                [
                    (1_000, "koji", Kind::Visibility, Api::GraphQl, Ended::Ok),
                    (1_030, "koji", Kind::DefaultBranch, Api::GraphQl, Ended::Ok),
                    (1_060, "koji", Kind::RepoLabels, Api::GraphQl, Ended::Failed),
                ]
            );
        }

        #[test]
        fn a_used_up_answer_is_recorded_and_the_calls_it_holds_are_recorded_as_held() {
            let rig = Rig::new();
            rig.forge.set_used_up(Some(Timestamp(1_600)));
            rig.held.visibility(&repo()).unwrap_err();
            rig.clock.advance(60);
            rig.held.viewer().unwrap_err();
            rig.forge.set_down(false);
            rig.clock.advance(600);
            rig.held.viewer().unwrap();

            let lines = rig.lines();
            let seen: Vec<_> = lines.iter().map(|l| (l.at.0, l.kind, l.ended)).collect();
            assert_eq!(
                seen,
                [
                    (1_000, Kind::Visibility, Ended::UsedUp),
                    (1_000, Kind::RateLimitReset, Ended::Ok),
                    (1_060, Kind::Viewer, Ended::Held),
                    (1_660, Kind::Viewer, Ended::Ok),
                ]
            );
            assert_eq!(lines[1].api, Api::Rest);
            assert_eq!(lines[1].millis, 0);
        }
    }
}
