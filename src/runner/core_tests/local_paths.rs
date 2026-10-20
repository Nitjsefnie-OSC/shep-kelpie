use std::path::Path;
use std::sync::Arc;

use crate::local_paths::LocalPaths;
use crate::ports::{ForgeError, GuardedVoice, Voice};
use crate::settings::ForgeSlug;

fn slug() -> ForgeSlug {
    ForgeSlug::try_from("shep-pm/lab".to_owned()).unwrap()
}

// The App's voice is guarded the same way, so an agent-ready body kelpie
// writes over an issue holds nothing of this machine's.
#[test]
fn the_apps_voice_refuses_an_issue_body_naming_a_local_path() {
    let home = crate::test::short_tempdir();
    let clock = crate::test::FakeClock::at(1_000_000);
    let github = crate::test::FakeGithub::new(clock.clone(), "shep-pm");
    github.registered(home.path(), "shep-pm/lab");
    let tokens = Arc::new(github.tokens(home.path()));
    let voice = GuardedVoice::new(
        Arc::new(crate::github::AppVoice::new(
            tokens,
            Box::new(github.clone()),
        )),
        LocalPaths::new([Path::new("/Users/me/.kelpie")]),
    );

    let err = voice
        .edit_issue_body(&slug(), 12, "Touches:\n- `/Users/me/.kelpie/notes.md`")
        .unwrap_err();
    assert!(matches!(err, ForgeError::LocalPath { .. }), "{err:?}");
    assert_eq!(github.issue_edits(), [], "nothing was sent");

    voice
        .edit_issue_body(&slug(), 12, "Touches:\n- `src/a.rs`")
        .unwrap();
    assert_eq!(
        github.issue_edits(),
        [(12, "Touches:\n- `src/a.rs`".to_owned())]
    );
}
