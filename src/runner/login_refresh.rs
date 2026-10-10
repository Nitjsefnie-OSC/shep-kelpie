//! Keeping a covered worker's login alive for as long as its call runs
//!
//! A worker's token lasts an hour from its mint, and a call can run longer:
//! `worker.turn_timeout` has no upper bound, and time spent waiting on the
//! test lease counts against it. The token is in a file `gh` reads at each
//! command, so a second thread writes a fresh one over it while the call runs.

use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::ports::Voice;
use crate::profile;
use crate::settings::ForgeSlug;

/// How long after the last write the next is made: half a token's hour
pub(super) const EVERY: Duration = Duration::from_secs(30 * 60);

/// Runs `run`, and while it does writes a fresh login into `build` every
/// `every`, when there is a `build` whose worker holds one
///
/// A mint that fails is tried again at the next time round: the file still
/// holds the last token, which has most of its hour left.
pub(super) fn while_running<T>(
    voice: &dyn Voice,
    repo: &ForgeSlug,
    build: Option<&Path>,
    every: Duration,
    run: impl FnOnce() -> T,
) -> T {
    let Some(build) = build else {
        return run();
    };
    let (stop, stopped) = mpsc::channel::<()>();
    thread::scope(|scope| {
        scope.spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                if let Ok(Some(token)) = voice.worker_token(repo) {
                    let _ = profile::write_login(build, &token);
                }
            }
        });
        // Dropped when `run` ends or unwinds, which ends the thread above.
        let _stop = stop;
        run()
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::github::AppVoice;
    use crate::test::{FakeClock, FakeGithub};

    fn token_in(build: &Path) -> String {
        let hosts = std::fs::read_to_string(build.join("gh/hosts.yml")).unwrap();
        let line = hosts.lines().find(|l| l.contains("oauth_token")).unwrap();
        line.rsplit(' ').next().unwrap().to_owned()
    }

    #[test]
    fn the_login_is_written_afresh_while_the_call_runs_and_not_after() {
        let (home, build) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let voice = AppVoice::new(
            Arc::new(github.tokens(home.path())),
            Box::new(github.clone()),
        );
        let repo = ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap();
        let first = voice.worker_token(&repo).unwrap().unwrap();
        profile::write_login(build.path(), &first).unwrap();

        while_running(
            &voice,
            &repo,
            Some(build.path()),
            Duration::from_millis(20),
            || std::thread::sleep(Duration::from_millis(400)),
        );

        let after = token_in(build.path());
        assert_ne!(after, first.expose(), "the call ran on its first token");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            token_in(build.path()),
            after,
            "written after the call ended"
        );
    }

    #[test]
    fn a_worker_with_no_login_is_left_alone() {
        let (home, build) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
        github.registered(home.path(), "shep-pm/koji");
        let voice = AppVoice::new(
            Arc::new(github.tokens(home.path())),
            Box::new(github.clone()),
        );
        let repo = ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap();

        let out = while_running(&voice, &repo, None, Duration::from_millis(1), || 7);

        assert_eq!(out, 7);
        assert!(github.asked().is_empty());
        assert!(!build.path().join("gh").exists());
    }
}
