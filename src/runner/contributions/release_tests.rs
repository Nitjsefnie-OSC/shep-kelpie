//! Asking a contributor for a release, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use super::pass_tests::read_through;
use super::ruling_tests::rule;
use super::security_tests::contribution;
use super::tests::poll;
use crate::ports::{Clock as _, Role};
use crate::runner::review_bot::contribution_turns::{FINDING, contributed, until_summoned};
use crate::runner::{Runner, StepReport, step};
use crate::test::{Rig, Scripted};

const DAY: u64 = Rig::DAY;

const ASKS: &str = "May kelpie take this from here? A review bot still has a thread open.";
const ASKS_AGAIN: &str = "One more time: may kelpie take this from here?";
const YES: &str = r#"{"reading": "yes"}"#;
const NO: &str = r#"{"reading": "no"}"#;
const OTHER: &str = r#"{"reading": "other"}"#;
const TAKEN: &str = "This pull request is kelpie's now. Your commits and credit stay.";
const THANKED: &str = "Thank you for the pull request. Kelpie is closing it now.";
const ANSWERS: &str = r#"{"kind": "answer", "text": "Yes, that is the failing test."}"#;

// Pull request 30 by `newcomer`, whose head a review bot found a thread in,
// so kelpie waits on the contributor from the last poll, with the
// representative saying `said` to each call, once each
fn waiting(said: &[&'static str]) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = contributed(&["cubic", "mine"]);
    until_summoned(&rig, &runner);
    let at = rig.clock.now().0 + 30;
    rig.forge.coderabbit.cubic_review(30, &head, at, &[FINDING]);
    read_through(&rig, &runner, &head);
    let said = said.iter().map(|s| Scripted::Say(s));
    rig.claude.script_for(Role::Representative, said);
    (rig, runner)
}

// Lets `days` pass, then polls
fn later(rig: &Rig, runner: &Mutex<Runner>, days: u64) {
    rig.clock.advance(days * DAY);
    poll(rig, runner);
}

// Polls three times, enough to hear what was written and deal with it
fn polls(rig: &Rig, runner: &Mutex<Runner>) {
    for _ in 0..3 {
        poll(rig, runner);
    }
}

fn held(rig: &Rig, runner: &Mutex<Runner>) -> Value {
    contribution(rig, runner)
}

// What the App posted on the pull requests
fn comments(rig: &Rig) -> Vec<String> {
    (rig.github.comments().into_iter())
        .map(|(_, body)| body)
        .collect()
}

// The prompts of the representative's calls
fn prompts(rig: &Rig) -> Vec<String> {
    let all = rig.claude.all_seen();
    (all.into_iter())
        .filter(|s| s.call.role == Role::Representative)
        .map(|s| s.call.prompt)
        .collect()
}

fn rulings(rig: &Rig, runner: &Mutex<Runner>) -> Vec<Value> {
    let status = rig.ask(runner, "status", None);
    status["rulings"].as_array().cloned().unwrap_or_default()
}

// The comment id of the latest ask
fn ask_id(rig: &Rig, runner: &Mutex<Runner>) -> u64 {
    held(rig, runner)["asked"]["comment"].as_u64().unwrap()
}

// A contribution that was asked, and said no, twice, and has been quiet
// since: its ruling waits for the maintainer
fn refused_twice() -> (Rig, Mutex<Runner>) {
    let (rig, runner) = waiting(&[ASKS, NO, ASKS_AGAIN, NO]);
    later(&rig, &runner, 3);
    rig.writes_on(30, "newcomer", "No thanks, I will finish it myself.");
    polls(&rig, &runner);
    later(&rig, &runner, 7);
    rig.writes_on(30, "newcomer", "Still no, sorry.");
    polls(&rig, &runner);
    assert_eq!(held(&rig, &runner)["asked"]["nos"], 2);
    later(&rig, &runner, 7);
    // The ruling reaches the webhook.
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));
    (rig, runner)
}

#[test]
fn at_nudge_days_one_comment_asks_for_a_release_and_names_open_bot_threads() {
    let (rig, runner) = waiting(&[ASKS]);
    let since = held(&rig, &runner)["waiting_since"].clone();
    later(&rig, &runner, 2);
    assert_eq!(comments(&rig), Vec::<String>::new(), "not yet the nudge");

    later(&rig, &runner, 1);
    polls(&rig, &runner);

    assert_eq!(comments(&rig), [ASKS], "one ask, however many polls");
    let asked = prompts(&rig);
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains("bleats"), "{}", asked[0]);
    assert!(asked[0].contains("take the pull request from here"));
    let record = held(&rig, &runner);
    assert!(record["asked"]["comment"].is_u64(), "{record}");
    assert_eq!(record["asked"]["asks"], 1);
    assert_eq!(record["waiting_since"], since, "the nudge keeps the clock");
    assert_eq!(record["released"], Value::Null);
}

#[test]
fn a_yes_reply_releases_the_contribution_and_the_representative_says_so() {
    let (rig, runner) = waiting(&[ASKS, YES, TAKEN]);
    later(&rig, &runner, 3);

    let wrote = rig.clock.now().0;
    rig.writes_on(30, "newcomer", "Yes, please go ahead.");
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["released"]["how"], "yes");
    let at = record["released"]["at"].as_u64().unwrap();
    assert!((wrote..=rig.clock.now().0).contains(&at), "{record}");
    assert_eq!(comments(&rig), [ASKS, TAKEN]);
    let asked = prompts(&rig);
    assert_eq!(asked.len(), 3, "the ask, the reading, the line: {asked:?}");
    assert!(asked[1].contains("Yes, please go ahead."), "{}", asked[1]);
    polls(&rig, &runner);
    assert_eq!(comments(&rig), [ASKS, TAKEN], "released once, said once");
}

#[test]
fn a_thumbs_up_on_the_ask_releases_it() {
    let (rig, runner) = waiting(&[ASKS, TAKEN]);
    later(&rig, &runner, 3);

    let id = ask_id(&rig, &runner);
    rig.forge.threads.thumbs(id, "newcomer", true);
    polls(&rig, &runner);

    assert_eq!(held(&rig, &runner)["released"]["how"], "thumbs-up");
    assert_eq!(comments(&rig), [ASKS, TAKEN]);
    assert_eq!(prompts(&rig).len(), 2, "no reply was read");
}

#[test]
fn silence_until_quiet_days_releases_it() {
    let (rig, runner) = waiting(&[ASKS, TAKEN]);
    later(&rig, &runner, 3);
    later(&rig, &runner, 3);
    assert_eq!(held(&rig, &runner)["released"], Value::Null);
    assert_eq!(comments(&rig), [ASKS]);

    later(&rig, &runner, 1);

    assert_eq!(held(&rig, &runner)["released"]["how"], "silence");
    assert_eq!(comments(&rig), [ASKS, TAKEN]);
}

#[test]
fn a_no_restarts_the_clock_and_a_second_ask_follows_a_second_quiet_window() {
    let (rig, runner) = waiting(&[ASKS, NO, ASKS_AGAIN]);
    later(&rig, &runner, 3);
    let first = ask_id(&rig, &runner);

    let wrote = rig.clock.now().0;
    rig.writes_on(30, "newcomer", "No thanks, I will finish it myself.");
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["asked"]["nos"], 1);
    assert_eq!(record["released"], Value::Null);
    let since = record["waiting_since"].as_u64().unwrap();
    assert!((wrote..=rig.clock.now().0).contains(&since), "{record}");
    later(&rig, &runner, 3);
    assert_eq!(comments(&rig), [ASKS], "the nudge does not ask again");

    later(&rig, &runner, 4);

    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN]);
    let record = held(&rig, &runner);
    assert_eq!(record["asked"]["asks"], 2);
    assert_ne!(record["asked"]["comment"], first);
    assert_eq!(record["waiting_since"], rig.clock.now().0, "a whole window");
    assert_eq!(record["released"], Value::Null);
}

#[test]
fn a_second_no_then_quiet_raises_a_ruling_with_keep_waiting_adopt_anyway_and_close() {
    let (rig, runner) = refused_twice();

    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    assert_eq!(waiting[0]["kind"]["kind"], "contribution");
    assert_eq!(waiting[0]["kind"]["reason"], "waiting");
    let question = waiting[0]["question"].as_str().unwrap();
    for option in ["keep waiting", "adopt anyway", "close with thanks"] {
        assert!(question.contains(option), "{option}: {question}");
    }
    assert_eq!(
        comments(&rig),
        [ASKS, ASKS_AGAIN],
        "nothing is posted for it"
    );
    assert_eq!(held(&rig, &runner)["released"], Value::Null);
    assert_eq!(rig.alerts.posts().len(), 1);
    polls(&rig, &runner);
    assert_eq!(rulings(&rig, &runner).len(), 1, "raised once");
}

#[test]
fn a_second_ask_met_with_silence_releases_it() {
    let (rig, runner) = waiting(&[ASKS, NO, ASKS_AGAIN, TAKEN]);
    later(&rig, &runner, 3);
    rig.writes_on(30, "newcomer", "No thanks, I will finish it myself.");
    polls(&rig, &runner);
    later(&rig, &runner, 7);
    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN]);
    later(&rig, &runner, 3);
    assert_eq!(held(&rig, &runner)["released"], Value::Null);

    later(&rig, &runner, 4);

    assert_eq!(held(&rig, &runner)["released"]["how"], "silence");
    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN, TAKEN]);
}

#[test]
fn keep_waiting_restarts_the_clock_and_the_count() {
    let (rig, runner) = refused_twice();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(ASKS)]);

    let asked_at = rig.clock.now().0;
    let ruled = rule(&rig, &runner, "30 answer keep waiting");
    assert_eq!(ruled.get("error"), None, "{ruled}");
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["asked"], Value::Null, "the count starts over");
    let since = record["waiting_since"].as_u64().unwrap();
    assert!(since >= asked_at && since <= rig.clock.now().0, "{record}");
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN]);
    later(&rig, &runner, 3);
    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN, ASKS], "asked afresh");
    assert_eq!(held(&rig, &runner)["asked"]["asks"], 1);
}

#[test]
fn adopt_anyway_records_a_release_by_the_maintainer() {
    let (rig, runner) = refused_twice();

    let ruled = rule(&rig, &runner, "30 answer adopt anyway");
    assert_eq!(ruled.get("error"), None, "{ruled}");
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["released"]["how"], "maintainer");
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN]);
    assert_eq!(rig.forge.pull_request_closings(), Vec::<u64>::new());
}

#[test]
fn close_with_thanks_posts_thanks_and_closes_the_pull_request() {
    let (rig, runner) = refused_twice();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(THANKED)]);

    let ruled = rule(&rig, &runner, "30 answer close with thanks");
    assert_eq!(ruled.get("error"), None, "{ruled}");
    polls(&rig, &runner);

    assert_eq!(comments(&rig), [ASKS, ASKS_AGAIN, THANKED]);
    assert_eq!(rig.forge.pull_request_closings(), [30]);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    polls(&rig, &runner);
    assert_eq!(comments(&rig).len(), 3, "thanked once");
    assert_eq!(rig.forge.pull_request_closings(), [30], "closed once");
    assert_eq!(step(&runner).unwrap(), None);
}

#[test]
fn a_reply_that_is_neither_yes_nor_no_is_a_conversation_and_not_a_release() {
    let (rig, runner) = waiting(&[ASKS, OTHER, ANSWERS]);
    later(&rig, &runner, 3);

    rig.writes_on(30, "newcomer", "Is that about the failing test?");
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["released"], Value::Null);
    assert_eq!(record["asked"]["nos"], Value::Null);
    assert_eq!(record["asked"]["read"].as_array().unwrap().len(), 1);
    assert_eq!(comments(&rig), [ASKS, "Yes, that is the failing test."]);
    assert_eq!(prompts(&rig).len(), 3, "the ask, the reading, the answer");
    polls(&rig, &runner);
    assert_eq!(prompts(&rig).len(), 3, "read once, answered once");
}

#[test]
fn a_thumbs_up_on_any_other_comment_is_not_a_release() {
    let (rig, runner) = waiting(&[ASKS]);
    later(&rig, &runner, 3);
    let id = ask_id(&rig, &runner);

    rig.forge.threads.thumbs(id + 1, "newcomer", true);
    rig.forge.threads.thumbs(id, "a-stranger", true);
    polls(&rig, &runner);

    let record = held(&rig, &runner);
    assert_eq!(record["released"], Value::Null);
    assert_eq!(record["asked"]["nos"], Value::Null);
    assert_eq!(comments(&rig), [ASKS]);
}
