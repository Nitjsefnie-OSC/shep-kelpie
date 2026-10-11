//! Kelpie's writes on GitHub, made with its App's installation token
//!
//! [`AppVoice`] is the [`Voice`] port over [`GithubApp`]'s tokens and
//! [`GithubApi`]'s writes. A repo whose owner has no App, or whose App is not
//! installed on it, is not covered, and kelpie writes there as it always has.

use std::sync::Arc;

use super::api::{self, ApiError, Call, GithubApi, ThreadWrite};
use super::tokens::{GithubApp, InstallationToken, TokenError};
use crate::ports::{ForgeError, NewLabel, PostedComment, Voice};
use crate::settings::ForgeSlug;

/// [`Voice`] as kelpie's App
pub struct AppVoice {
    tokens: Arc<dyn GithubApp>,
    api: Box<dyn GithubApi>,
}

impl AppVoice {
    /// A voice that writes through `api` with the tokens `tokens` mints
    pub fn new(tokens: Arc<dyn GithubApp>, api: Box<dyn GithubApi>) -> Self {
        Self { tokens, api }
    }

    fn token(&self, repo: &ForgeSlug) -> Result<InstallationToken, ForgeError> {
        self.tokens
            .token(repo)
            .map_err(|e| ForgeError::App(e.to_string()))
    }

    fn write(&self, repo: &ForgeSlug, call: &Call) -> Result<String, ForgeError> {
        let token = self.token(repo)?;
        self.api
            .write(&token, call)
            .map_err(|e| ForgeError::App(e.to_string()))
    }

    // GraphQL answers a refusal with HTTP 200, so the body says how it went.
    fn thread_write(
        &self,
        repo: &ForgeSlug,
        call: &Call,
        mutation: &'static str,
    ) -> Result<(), ForgeError> {
        let answer = self.write(repo, call)?;
        match api::parse_thread_write(&answer, mutation) {
            Ok(ThreadWrite::Done | ThreadWrite::Gone) => Ok(()),
            Ok(ThreadWrite::Refused(why)) => Err(ForgeError::App(format!(
                "GitHub refused the {mutation}: {why}"
            ))),
            Err(e) => Err(ForgeError::App(e.to_string())),
        }
    }
}

impl std::fmt::Debug for AppVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppVoice").finish_non_exhaustive()
    }
}

impl Voice for AppVoice {
    // An App that is set but cannot mint a token now, such as with GitHub
    // out of reach, still covers the repo: its writes fail and are logged,
    // and nothing is posted as the maintainer in its place.
    fn covers(&self, repo: &ForgeSlug) -> bool {
        !matches!(
            self.tokens.token(repo),
            Err(TokenError::Owner(_) | TokenError::NoApp(_) | TokenError::NotInstalled { .. })
        )
    }

    fn worker_token(&self, repo: &ForgeSlug) -> Result<Option<InstallationToken>, ForgeError> {
        match self.tokens.worker_token(repo) {
            Ok(token) => Ok(Some(token)),
            Err(TokenError::Owner(_) | TokenError::NoApp(_) | TokenError::NotInstalled { .. }) => {
                Ok(None)
            }
            // GitHub out of reach, busy or rate limited: asking again later may work.
            Err(TokenError::Api(e)) if e.passes() => Err(ForgeError::AppBusy(e.to_string())),
            Err(e) => Err(ForgeError::App(e.to_string())),
        }
    }

    fn comment(&self, repo: &ForgeSlug, thread: u64, body: &str) -> Result<(), ForgeError> {
        self.write(repo, &Call::comment(repo, thread, body))
            .map(drop)
    }

    fn post(&self, repo: &ForgeSlug, thread: u64, body: &str) -> Result<PostedComment, ForgeError> {
        let answer = self.write(repo, &Call::comment(repo, thread, body))?;
        let (id, created) =
            api::parse_posted(&answer).map_err(|e| ForgeError::App(e.to_string()))?;
        Ok(PostedComment { id, created })
    }

    fn thumbs_up(&self, repo: &ForgeSlug, comment: u64) -> Result<(), ForgeError> {
        self.write(repo, &Call::thumbs_up(repo, comment)).map(drop)
    }

    fn create_issue(
        &self,
        repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        let answer = self.write(repo, &Call::issue(repo, title, body, labels))?;
        api::parse_number(&answer).map_err(|e| ForgeError::App(e.to_string()))
    }

    fn resolve_thread(&self, repo: &ForgeSlug, thread: &str) -> Result<(), ForgeError> {
        self.thread_write(repo, &Call::resolve_thread(thread), "resolveReviewThread")
    }

    fn reply_to_thread(
        &self,
        repo: &ForgeSlug,
        thread: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        let call = Call::reply_to_thread(thread, body);
        self.thread_write(repo, &call, "addPullRequestReviewThreadReply")
    }

    fn create_label(&self, repo: &ForgeSlug, label: &NewLabel) -> Result<(), ForgeError> {
        self.write(repo, &Call::label(repo, label)).map(drop)
    }

    fn set_issue_label(
        &self,
        repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        if on {
            return self
                .write(repo, &Call::put_label(repo, number, label))
                .map(drop);
        }
        let call = Call::take_label(repo, number, label);
        let token = self.token(repo)?;
        match self.api.write(&token, &call) {
            // A label the issue does not carry is as good as taken off.
            Ok(_) | Err(ApiError::Refused(404)) => Ok(()),
            Err(e) => Err(ForgeError::App(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests;
