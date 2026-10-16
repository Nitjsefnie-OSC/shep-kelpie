//! A quiet contribution kelpie cannot push to, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use super::release_tests::{ASKS, TAKEN, THANKED, comments, later, polls, rulings, waiting};
use super::ruling_tests::rule;
use crate::runner::Runner;
use crate::test::Rig;

// A contribution asked once and quiet since, on a fork whose owner allows
// edits only if `edits`, one day short of `quiet_days`
fn nearly_quiet(edits: bool) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = waiting(&[ASKS, if edits { TAKEN } else { THANKED }]);
    rig.forge
        .set_fork(30, "https://example.test/fork.git", edits);
    later(&rig, &runner, 3);
    later(&rig, &runner, 3);
    (rig, runner)
}

// A contribution on a fork without edits, quiet for `quiet_days`: its ruling
// waits for the maintainer
fn without_edits() -> (Rig, Mutex<Runner>) {
    let (rig, runner) = nearly_quiet(false);
    later(&rig, &runner, 1);
    (rig, runner)
}

fn followed(runner: &Mutex<Runner>) -> Vec<u64> {
    let state = runner.lock().unwrap().state.clone();
    state.contributions.iter().map(|c| c.pull_request).collect()
}

#[test]
fn a_fork_without_maintainer_edits_raises_the_leave_open_or_close_ruling_at_quiet_days() {
    let (rig, runner) = nearly_quiet(false);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new(), "not yet quiet");

    later(&rig, &runner, 1);

    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0]["kind"]["kind"], "contribution");
    assert_eq!(waiting[0]["kind"]["reason"], "no-edits");
    let question = waiting[0]["question"].as_str().unwrap();
    for option in ["leave it open", "close with thanks"] {
        assert!(question.contains(option), "{option}: {question}");
    }
    assert_eq!(comments(&rig), [ASKS], "nothing is posted for it");
    assert_eq!(followed(&runner), [30], "still followed until it is ruled");
    polls(&rig, &runner);
    assert_eq!(rulings(&rig, &runner).len(), 1, "raised once");
}

#[test]
fn a_fork_with_maintainer_edits_is_released_at_quiet_days_as_before() {
    let (rig, runner) = nearly_quiet(true);
    later(&rig, &runner, 1);

    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    let record = super::release_tests::held(&rig, &runner);
    assert_eq!(record["released"]["how"], "silence");
}

#[test]
fn leave_open_stops_watching_it_and_posts_nothing() {
    let (rig, runner) = without_edits();

    let ruled = rule(&rig, &runner, "30 answer leave it open");
    assert_eq!(ruled.get("error"), None, "{ruled}");
    polls(&rig, &runner);

    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    assert_eq!(comments(&rig), [ASKS], "nothing was posted");
    assert_eq!(rig.forge.pull_request_closings(), Vec::<u64>::new());
    assert_eq!(followed(&runner), Vec::<u64>::new(), "no longer followed");
    later(&rig, &runner, 30);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    assert_eq!(comments(&rig), [ASKS], "still nothing");
    assert_eq!(followed(&runner), Vec::<u64>::new(), "not followed afresh");
}

#[test]
fn close_with_thanks_posts_thanks_and_closes_it() {
    let (rig, runner) = without_edits();

    let ruled = rule(&rig, &runner, "30 answer close with thanks");
    assert_eq!(ruled.get("error"), None, "{ruled}");
    polls(&rig, &runner);

    assert_eq!(comments(&rig), [ASKS, THANKED]);
    assert_eq!(rig.forge.pull_request_closings(), [30]);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    polls(&rig, &runner);
    assert_eq!(comments(&rig).len(), 2, "thanked once");
    assert_eq!(rig.forge.pull_request_closings(), [30], "closed once");
}
