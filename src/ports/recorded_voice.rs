//! Recording each of kelpie's App's calls in the forge ledger

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use super::{Clock, ForgeError, NewLabel, PostedComment, Voice};
use crate::forge_ledger::{Ended, Kind, Ledger};
use crate::github::InstallationToken;
use crate::settings::ForgeSlug;

/// A voice whose calls are each added to the forge ledger as they end
///
/// [`Voice::covers`] is not a call: it is answered from the App's cached
/// installations, and a refresh it makes is its token's mint, which the
/// App's token calls count.
pub struct RecordedVoice {
    voice: Arc<dyn Voice>,
    clock: Arc<dyn Clock>,
    ledger: Arc<Ledger>,
}

impl RecordedVoice {
    /// `voice`, adding each call to `ledger` at `clock`'s time
    pub fn new(voice: Arc<dyn Voice>, clock: Arc<dyn Clock>, ledger: Arc<Ledger>) -> Self {
        Self {
            voice,
            clock,
            ledger,
        }
    }

    fn call<T>(
        &self,
        kind: Kind,
        call: impl FnOnce(&dyn Voice) -> Result<T, ForgeError>,
    ) -> Result<T, ForgeError> {
        let started = Instant::now();
        let result = call(self.voice.as_ref());
        let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let ended = match &result {
            Ok(_) => Ended::Ok,
            Err(error) if rate_limited(error) => Ended::UsedUp,
            Err(_) => Ended::Failed,
        };
        self.ledger.record(self.clock.now(), kind, millis, ended);
        result
    }
}

/// Whether the App's call failed on a rate limit, which its errors say in words
fn rate_limited(error: &ForgeError) -> bool {
    matches!(error, ForgeError::Limited { .. })
        || matches!(error, ForgeError::App(why) | ForgeError::AppBusy(why)
            if why.to_lowercase().contains("rate limit"))
}

impl fmt::Debug for RecordedVoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordedVoice").finish_non_exhaustive()
    }
}

impl Voice for RecordedVoice {
    fn covers(&self, repo: &ForgeSlug) -> bool {
        self.voice.covers(repo)
    }

    fn worker_token(&self, repo: &ForgeSlug) -> Result<Option<InstallationToken>, ForgeError> {
        self.call(Kind::AppWorkerToken, |v| v.worker_token(repo))
    }

    fn comment(&self, repo: &ForgeSlug, thread: u64, body: &str) -> Result<(), ForgeError> {
        self.call(Kind::AppComment, |v| v.comment(repo, thread, body))
    }

    fn post(&self, repo: &ForgeSlug, thread: u64, body: &str) -> Result<PostedComment, ForgeError> {
        self.call(Kind::AppComment, |v| v.post(repo, thread, body))
    }

    fn edit_comment(&self, repo: &ForgeSlug, comment: u64, body: &str) -> Result<(), ForgeError> {
        self.call(Kind::AppEditComment, |v| {
            v.edit_comment(repo, comment, body)
        })
    }

    fn thumbs_up(&self, repo: &ForgeSlug, comment: u64) -> Result<(), ForgeError> {
        self.call(Kind::AppThumbsUp, |v| v.thumbs_up(repo, comment))
    }

    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        self.call(Kind::AppCreateIssue, |v| {
            v.create_issue(repo, title, body, labels)
        })
    }

    fn resolve_thread(&self, repo: &ForgeSlug, thread: &str) -> Result<(), ForgeError> {
        self.call(Kind::AppResolveThread, |v| v.resolve_thread(repo, thread))
    }

    fn reply_to_thread(
        &self,
        repo: &ForgeSlug,
        thread: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        self.call(Kind::AppReplyToThread, |v| {
            v.reply_to_thread(repo, thread, body)
        })
    }

    fn create_label(&self, repo: &ForgeSlug, label: &NewLabel) -> Result<(), ForgeError> {
        self.call(Kind::AppCreateLabel, |v| v.create_label(repo, label))
    }

    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        self.call(Kind::AppSetIssueLabel, |v| {
            v.set_issue_label(repo, number, label, on)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge_ledger::{self, Api};
    use crate::github::{ApiError, AppVoice};
    use crate::ports::Cooldown;
    use crate::test::{FakeClock, FakeGithub};

    fn koji() -> ForgeSlug {
        ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
    }

    #[test]
    fn each_app_call_adds_a_line_with_its_own_kind_and_how_it_ended() {
        let home = tempfile::tempdir().unwrap();
        let clock = FakeClock::at(1_000_000);
        let github = FakeGithub::new(clock.clone(), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let tokens = Arc::new(github.tokens(home.path()));
        let voice = RecordedVoice::new(
            Arc::new(AppVoice::new(tokens, Box::new(github.clone()))),
            Arc::new(clock.clone()),
            Arc::new(Ledger::in_home(home.path(), "koji")),
        );

        voice.comment(&koji(), 7, "hello").unwrap();
        clock.advance(10);
        voice.thumbs_up(&koji(), 1).unwrap();
        clock.advance(10);
        github.refuse_comments(ApiError::RateLimited(Cooldown::default()));
        voice.comment(&koji(), 7, "again").unwrap_err();

        let lines = forge_ledger::read(&home.path().join(forge_ledger::FILE)).unwrap();
        let seen: Vec<_> = lines
            .iter()
            .map(|l| (l.at.0, l.project.as_str(), l.kind, l.api, l.ended))
            .collect();
        assert_eq!(
            seen,
            [
                (1_000_000, "koji", Kind::AppComment, Api::Rest, Ended::Ok),
                (1_000_010, "koji", Kind::AppThumbsUp, Api::Rest, Ended::Ok),
                (
                    1_000_020,
                    "koji",
                    Kind::AppComment,
                    Api::Rest,
                    Ended::UsedUp
                ),
            ]
        );
    }
}
