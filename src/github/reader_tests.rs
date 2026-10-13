use super::*;
use crate::ports::Timestamp;
use crate::test::{Asked, FakeClock, FakeGithub};

fn slug(s: &str) -> ForgeSlug {
    ForgeSlug::try_from(s.to_owned()).unwrap()
}

fn requested(github: &FakeGithub) -> Vec<String> {
    (github.asked().into_iter())
        .filter_map(|asked| match asked {
            Asked::AccessToken(_, body) => Some(body),
            _ => None,
        })
        .collect()
}

#[test]
fn a_reading_token_asks_to_read_issues_pull_requests_checks_and_statuses_and_nothing_else() {
    let koji = slug("shep-pm/koji");

    assert_eq!(
        api::token_request(&koji, Grant::Reader),
        r#"{"permissions":{"checks":"read","issues":"read","pull_requests":"read","statuses":"read"},"repositories":["koji"]}"#
    );
    assert_eq!(
        api::token_request(&koji, Grant::ReaderCore),
        r#"{"permissions":{"issues":"read","pull_requests":"read"},"repositories":["koji"]}"#
    );
}

#[test]
fn a_reading_token_is_kept_apart_from_the_posting_one_and_minted_again_near_its_end() {
    let home = tempfile::tempdir().unwrap();
    let clock = FakeClock::at(1_000_000);
    let github = FakeGithub::new(clock.clone(), "shep-pm");
    github.registered(home.path(), "shep-pm/koji");
    let tokens = github.tokens(home.path());
    let koji = slug("shep-pm/koji");

    let posts = tokens.token(&koji).unwrap();
    let reads = tokens.reader(&koji).unwrap();
    assert_ne!(reads.token, posts, "a read never borrows the posting token");
    assert_eq!(reads.permissions["pull_requests"], "read");
    assert_eq!(tokens.reader(&koji).unwrap(), reads, "kept");
    assert_eq!(
        tokens.token(&koji).unwrap(),
        posts,
        "the posting token is still its own"
    );
    assert_eq!(requested(&github).len(), 2);

    clock.advance(FakeGithub::LIFETIME - 5 * 60);
    let again = tokens.reader(&koji).unwrap();

    assert_ne!(again.token, reads.token);
    assert_eq!(
        again.expires_at,
        Timestamp(1_000_000 + 2 * FakeGithub::LIFETIME - 5 * 60)
    );
}

#[test]
fn an_app_never_given_checks_or_statuses_still_reads_issues_and_pull_requests() {
    let home = tempfile::tempdir().unwrap();
    let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
    github.registered(home.path(), "shep-pm/koji");
    github.never_given(&["checks", "statuses"]);
    let tokens = github.tokens(home.path());
    let koji = slug("shep-pm/koji");

    let reads = tokens.reader(&koji).unwrap();

    assert_eq!(reads.permissions["issues"], "read");
    assert_eq!(reads.permissions["pull_requests"], "read");
    assert!(!reads.permissions.contains_key("checks"));
    assert_eq!(
        requested(&github).len(),
        2,
        "the full ask, then the core one"
    );

    // The token that came is kept, so the full ask is not repeated on every read.
    tokens.reader(&koji).unwrap();
    assert_eq!(requested(&github).len(), 2);
}

#[test]
fn no_app_for_the_owner_or_no_install_gives_no_reading_token() {
    let home = tempfile::tempdir().unwrap();
    let github = FakeGithub::new(FakeClock::at(1_000_000), "shep-pm");
    github.registered(home.path(), "shep-pm/koji");
    let tokens = github.tokens(home.path());

    assert!(matches!(
        tokens.reader(&slug("shep-pm/other")),
        Err(TokenError::NotInstalled { .. })
    ));
    assert!(matches!(
        tokens.reader(&slug("someone/else")),
        Err(TokenError::NoApp(_))
    ));
}
