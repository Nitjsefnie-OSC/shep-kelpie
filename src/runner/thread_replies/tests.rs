use std::sync::Mutex;

use crate::runner::{CHECKS_SETTLE, Runner, step};
use crate::test::Rig;

// A merge ruling for pull request 71 is up through the App, which mentions
// `octocat`, who has write access to the repo.
fn a_merge_ruling_on_71() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.forge.threads.grant_write("octocat");
    // The ruling went up some seconds ago.
    rig.clock.advance(5);
    (rig, runner, head)
}

#[test]
fn a_yes_from_a_login_with_write_access_merges_the_pull_request() {
    let (rig, runner, head) = a_merge_ruling_on_71();
    let comment = rig.writes_on(71, "octocat", "yes");

    rig.next_look();
    step(&runner).unwrap(); // takes the reply, and marks the pull request ready
    rig.clock.advance(CHECKS_SETTLE);
    step(&runner).unwrap(); // merges

    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.github.reactions(), [comment], "kelpie took the reply");
}
