//! The reads that run as kelpie's GitHub App
//!
//! `gh` reads as whichever login its config folder holds. For a repo the App
//! covers, the folder is one of kelpie's own, in its home, holding the App's
//! reading token in the `hosts.yml` `gh` reads, as a worker's folder holds
//! its token. The token is written again only when it changes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use super::{board, issue, issues, pull_request, review, review_bot};
use crate::board::{OpenPullRequest, ReadyIssue};
use crate::github::{GithubApp, InstallationToken};
use crate::ports::{
    AppForges, AppReads, ForgeError, Issue, PullRequest, Reading, Reviewed, SubIssue,
};
use crate::profile::{login_dir, write_login};
use crate::review_bot::{Activity, Login};
use crate::settings::ForgeSlug;

/// The reads of one repo, as the App, through the `gh` config folder `config`
#[derive(Debug)]
pub struct GhAppReads {
    config: PathBuf,
}

impl AppReads for GhAppReads {
    fn issue(&self, repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError> {
        issue(Some(&self.config), repo, number)
    }

    fn sub_issues(&self, repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
        issues::sub_issues(Some(&self.config), repo, parent)
    }

    fn ready_issues(&self, repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        board::ready_issues(Some(&self.config), repo)
    }

    fn open_pull_requests(&self, repo: &ForgeSlug) -> Result<Vec<OpenPullRequest>, ForgeError> {
        board::open_pull_requests(Some(&self.config), repo)
    }

    fn pull_request(&self, repo: &ForgeSlug, number: u64) -> Result<PullRequest, ForgeError> {
        pull_request(Some(&self.config), repo, number)
    }

    fn reviewed(&self, repo: &ForgeSlug, number: u64) -> Result<Reviewed, ForgeError> {
        review::reviewed(Some(&self.config), repo, number)
    }

    fn review_bot(
        &self,
        repo: &ForgeSlug,
        number: u64,
        login: Login<'_>,
    ) -> Result<Activity, ForgeError> {
        review_bot::activity(Some(&self.config), repo, number, login)
    }
}

/// The repos the App covers, and the `gh` login that reads each
pub struct GhApps {
    tokens: Arc<dyn GithubApp>,
    // Where each repo's config folder is, in kelpie's home
    folders: PathBuf,
    // The token each folder holds, so a token kept is not written again
    written: Mutex<BTreeMap<PathBuf, InstallationToken>>,
}

impl GhApps {
    /// Reads through `tokens`, with the config folders under `folders`
    pub fn new(tokens: Arc<dyn GithubApp>, folders: &Path) -> Self {
        Self {
            tokens,
            folders: folders.to_owned(),
            written: Mutex::new(BTreeMap::new()),
        }
    }

    // One folder for a repo: the slug's one slash is the only thing that
    // cannot be in a file name.
    fn folder(&self, repo: &ForgeSlug) -> PathBuf {
        self.folders
            .join(repo.as_str().to_ascii_lowercase().replace('/', "--"))
    }
}

impl std::fmt::Debug for GhApps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GhApps")
            .field("folders", &self.folders)
            .finish_non_exhaustive()
    }
}

impl AppForges for GhApps {
    fn reading(&self, repo: &ForgeSlug) -> Reading {
        // No App, one not installed on the repo, or a token that cannot be
        // minted: the login reads, and `doctor` says why.
        let Ok(issued) = self.tokens.reader(repo) else {
            return Reading::Login;
        };
        let build = self.folder(repo);
        let mut written = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        if written.get(&build) != Some(&issued.token) {
            if write_login(&build, &issued.token).is_err() {
                return Reading::Login;
            }
            written.insert(build.clone(), issued.token.clone());
        }
        let holds: BTreeSet<String> = (issued.permissions.into_iter())
            .filter(|(_, level)| matches!(level.as_str(), "read" | "write"))
            .map(|(name, _)| name)
            .collect();
        Reading::App {
            reads: Arc::new(GhAppReads {
                config: login_dir(&build),
            }),
            holds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{FakeClock, FakeGithub};

    fn slug(s: &str) -> ForgeSlug {
        ForgeSlug::try_from(s.to_owned()).unwrap()
    }

    fn hosts(apps: &GhApps, repo: &ForgeSlug) -> String {
        let hosts = login_dir(&apps.folder(repo)).join("hosts.yml");
        std::fs::read_to_string(hosts).unwrap()
    }

    #[test]
    fn a_covered_repo_is_read_through_a_folder_holding_the_apps_reading_token() {
        let home = tempfile::tempdir().unwrap();
        let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let tokens = Arc::new(github.tokens(home.path()));
        let apps = GhApps::new(tokens.clone(), &home.path().join("reads"));
        let koji = slug("shep-pm/koji");

        let Reading::App { holds, .. } = apps.reading(&koji) else {
            panic!("the App covers shep-pm/koji");
        };

        let reading = tokens.reader(&koji).unwrap();
        assert!(hosts(&apps, &koji).contains(reading.token.expose()));
        for name in ["issues", "pull_requests", "checks", "statuses"] {
            assert!(holds.contains(name), "{name} in {holds:?}");
        }
    }

    #[test]
    fn a_repo_the_app_is_not_installed_on_is_read_by_the_login() {
        let home = tempfile::tempdir().unwrap();
        let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let apps = GhApps::new(
            Arc::new(github.tokens(home.path())),
            &home.path().join("reads"),
        );

        assert!(matches!(
            apps.reading(&slug("shep-pm/golbat")),
            Reading::Login
        ));
        assert!(!home.path().join("reads").join("shep-pm--golbat").exists());
    }

    #[test]
    fn a_token_that_lapsed_is_replaced_in_the_folder_and_only_then() {
        let home = tempfile::tempdir().unwrap();
        let clock = FakeClock::at(1_000_000);
        let github = FakeGithub::new(clock.clone(), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let apps = GhApps::new(
            Arc::new(github.tokens(home.path())),
            &home.path().join("reads"),
        );
        let koji = slug("shep-pm/koji");

        apps.reading(&koji);
        let first = hosts(&apps, &koji);
        apps.reading(&koji);
        assert_eq!(hosts(&apps, &koji), first);
        clock.advance(FakeGithub::LIFETIME - 5 * 60);
        apps.reading(&koji);

        assert_ne!(hosts(&apps, &koji), first);
    }

    #[test]
    fn an_app_never_given_checks_holds_the_rest_and_not_checks() {
        let home = tempfile::tempdir().unwrap();
        let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        github.never_given(&["checks", "statuses"]);
        let apps = GhApps::new(
            Arc::new(github.tokens(home.path())),
            &home.path().join("reads"),
        );

        let Reading::App { holds, .. } = apps.reading(&slug("shep-pm/koji")) else {
            panic!("the App covers shep-pm/koji");
        };

        assert!(holds.contains("pull_requests"));
        assert!(!holds.contains("checks"));
        assert!(!holds.contains("statuses"));
    }
}
