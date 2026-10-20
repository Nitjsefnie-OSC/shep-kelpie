//! Two runners on one login, one fake clock: the App's calls and `gh`'s share a hold

use crate::github::{ApiError, AppVoice};
use crate::ports::paced_voice::*;
use crate::ports::{CallPacer, Cooldown, Forge, LocalPace, Pace, RateHeld, Tickets, Timestamp};
use crate::ports::{Clock, ForgeError, ForgeHold, Voice};
use crate::settings::ForgeSlug;
use crate::test::{FakeClock, FakeForge, FakeGithub};
use std::sync::Arc;

fn koji() -> ForgeSlug {
    ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap()
}

// A runner's App on `clock`, held and spaced by `hold`.
fn app(
    github: &FakeGithub,
    home: &std::path::Path,
    clock: &FakeClock,
    hold: &ForgeHold,
    project: &str,
) -> PacedVoice {
    let tokens = Arc::new(github.tokens(home));
    PacedVoice::new(
        Arc::new(AppVoice::new(tokens, Box::new(github.clone()))),
        Arc::new(clock.clone()),
        hold.clone(),
        project,
        Box::new(|_| {}),
    )
}

#[test]
fn an_app_calls_retry_after_holds_the_other_runners_app_and_gh_calls_for_that_long() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::new()));
    let (one, two) = (
        FakeGithub::new(clock.clone(), "shep-pm"),
        FakeGithub::new(clock.clone(), "shep-pm"),
    );
    one.registered(home.path(), "shep-pm/koji");
    two.registered(home.path(), "shep-pm/koji");
    let first = app(&one, home.path(), &clock, &hold, "koji");
    let second = app(&two, home.path(), &clock, &hold, "golbat");
    let forge = FakeForge::new(home.path().join("origin"));
    let source = Arc::new(forge.clone());
    let gh = RateHeld::new(
        Arc::clone(&source) as Arc<dyn Forge>,
        Arc::clone(&source) as Arc<dyn Tickets>,
        Arc::new(clock.clone()),
        hold.clone(),
        Box::new(|_| {}),
    );
    one.refuse_comments(ApiError::RateLimited(Cooldown {
        retry_after: Some(45),
        reset: None,
    }));

    first.comment(&koji(), 7, "hello").unwrap_err();

    assert_eq!(
        second.comment(&koji(), 7, "hello").unwrap_err(),
        ForgeError::Held(Timestamp(1_000_045))
    );
    assert_eq!(
        gh.viewer().unwrap_err(),
        ForgeError::Held(Timestamp(1_000_045))
    );
    assert_eq!(forge.asked(), 0, "gh was not asked");

    clock.advance(45);
    second.comment(&koji(), 7, "hello").unwrap();
    gh.viewer().unwrap();
}

#[test]
fn a_hold_the_app_set_ends_with_the_pace_its_calls_were_spaced_by() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let pace = Arc::new(LocalPace::with(CallPacer::new(2, 5)));
    let hold = ForgeHold::shared(pace.clone());
    let github = FakeGithub::new(clock.clone(), "shep-pm");
    github.registered(home.path(), "shep-pm/koji");
    let voice = app(&github, home.path(), &clock, &hold, "koji");

    voice.thumbs_up(&koji(), 1).unwrap();
    voice.thumbs_up(&koji(), 1).unwrap();
    // The third waits out the minute on the clock, then goes.
    voice.thumbs_up(&koji(), 1).unwrap();

    assert_eq!(clock.now(), Timestamp(1_000_060));
    assert_eq!(pace.held(clock.now()), None);
}

#[test]
fn a_repo_no_app_covers_gets_no_token_and_is_not_stopped_by_a_hold() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::new()));
    hold.hold(Timestamp(1_000_600));
    let github = FakeGithub::new(clock.clone(), "shep-pm");
    let voice = app(&github, home.path(), &clock, &hold, "koji");

    assert_eq!(voice.worker_token(&koji()).unwrap(), None);
}

#[test]
fn a_repo_an_app_covers_waits_out_a_hold_for_its_worker_token() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let hold = ForgeHold::shared(Arc::new(LocalPace::new()));
    hold.hold(Timestamp(1_000_600));
    let github = FakeGithub::new(clock.clone(), "shep-pm");
    github.registered(home.path(), "shep-pm/koji");
    github.accept_workers();
    let voice = app(&github, home.path(), &clock, &hold, "koji");

    assert_eq!(
        voice.worker_token(&koji()).unwrap_err(),
        ForgeError::Held(Timestamp(1_000_600))
    );
}
