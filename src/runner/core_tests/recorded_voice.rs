use crate::forge_ledger::{self, Api};
use crate::forge_ledger::{Ended, Kind, Ledger};
use crate::github::{ApiError, AppVoice};
use crate::ports::Cooldown;
use crate::ports::Voice;
use crate::ports::recorded_voice::*;
use crate::settings::ForgeSlug;
use crate::test::{FakeClock, FakeGithub};
use std::sync::Arc;

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
