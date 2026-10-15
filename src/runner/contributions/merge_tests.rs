//! The merge ruling of a contribution, through the runner's stand-ins

use std::sync::Mutex;

use serde_json::Value;

use super::pass_tests::{listing, read_through, write};
use super::ruling_tests::rule;
use super::security_tests::{commit_on, contribute, contribution};
use super::tests::poll;
use crate::ports::{AgentError, CLEAN, Finding, PullRequestState, Role, Severity};
use crate::runner::words::NO_NOTE;
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted, ScriptedRound};

const PASSED: &str = "The local reviewer found nothing. The maintainer has it now.";
const THANKS: &str = "Thank you for the pull request. It is merged now.";
const DECLINE: &str = "The maintainer would like a test first, please add one.";

// Pull request 30, whose head passed the security review, whose CI is green
// and whose pass over it has ended, with the ruling not yet looked at
fn reviewed() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    // The pass of a clean head says so to the contributor.
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(PASSED)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);
    (rig, runner, head)
}

// `reviewed`, with the merge ruling raised and its alert sent
fn ruled_on() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, head) = reviewed();
    poll(&rig, &runner);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));
    (rig, runner, head)
}

fn rulings(rig: &Rig, runner: &Mutex<Runner>) -> Vec<Value> {
    let status = rig.ask(runner, "status", None);
    status["rulings"].as_array().cloned().unwrap_or_default()
}

fn found() -> Finding {
    Finding::new(
        Severity::Medium,
        "one.txt",
        1,
        "leftover placeholder",
        "it ships to users",
    )
}

#[test]
fn the_merge_ruling_is_raised_when_the_pass_has_ended_and_ci_is_green_on_the_head() {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    rig.reviewer
        .script([ScriptedRound::Findings(vec![found()])]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);
    assert_eq!(
        rulings(&rig, &runner),
        Vec::<Value>::new(),
        "not before the poll"
    );

    // The poll after the pass raises it, and the step after that alerts.
    poll(&rig, &runner);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));

    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1);
    let ruling = &waiting[0];
    assert_eq!(ruling["id"], 30);
    assert_eq!(ruling["issue"], Value::Null);
    assert_eq!(ruling["pull_request"], 30);
    assert_eq!(ruling["kind"]["kind"], "contribution");
    assert_eq!(ruling["kind"]["reason"], "merge");
    assert_eq!(ruling["kind"]["head"], head);
    let question = ruling["question"].as_str().unwrap();
    for named in [
        "leftover placeholder",
        "one.txt:1",
        "shep kelpie rule 30 yes",
    ] {
        assert!(question.contains(named), "{named}: {question}");
    }

    // Once per head: later polls raise no other, alert nothing more, merge nothing.
    poll(&rig, &runner);
    poll(&rig, &runner);
    assert_eq!(rulings(&rig, &runner).len(), 1);
    assert_eq!(rig.alerts.posts().len(), 1);
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn it_is_raised_under_git_merging_auto_too_and_sends_no_notice() {
    let (rig, runner) = listing(&["mine"]);
    drop(runner);
    rig.merge_auto();
    let runner = rig.open().unwrap();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);

    poll(&rig, &runner);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));
    poll(&rig, &runner);

    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["kind"]["reason"], "merge");
    assert_eq!(
        rig.forge.merges(),
        [],
        "nothing merges until the maintainer says"
    );
    // The one alert is the ruling's: no notice of a merge.
    let alerted = rig.alerts.posts();
    assert_eq!(alerted.len(), 1, "{alerted:?}");
    assert!(
        alerted[0].1.title.contains("ruling #30"),
        "{:?}",
        alerted[0].1
    );
}

#[test]
fn a_yes_merges_the_head_it_named() {
    let (rig, runner, head) = ruled_on();

    rule(&rig, &runner, "30 yes");
    poll(&rig, &runner);

    assert_eq!(rig.forge.merges(), [(30, head)]);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());

    // It merges once, however many polls follow.
    poll(&rig, &runner);
    assert_eq!(rig.forge.merges().len(), 1);
}

fn down() -> AgentError {
    AgentError::Failed(Harness::ClaudeCode, "overloaded".into())
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

#[test]
fn a_merge_posts_thanks_in_the_representatives_words() {
    let (rig, runner, _) = ruled_on();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(THANKS)]);

    rule(&rig, &runner, "30 yes");
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(
        comments(&rig),
        [PASSED, THANKS],
        "one thanks, however many polls"
    );
    let asked = prompts(&rig);
    assert_eq!(asked.len(), 2);
    assert!(asked[1].contains("merged"), "{}", asked[1]);
    assert!(asked[1].contains("@newcomer"), "{}", asked[1]);
}

#[test]
fn a_failed_thanks_call_does_not_hold_the_merge() {
    let (rig, runner, head) = ruled_on();
    rig.claude
        .script_for(Role::Representative, [Scripted::Fail(down())]);

    rule(&rig, &runner, "30 yes");
    poll(&rig, &runner);

    assert_eq!(rig.forge.merges(), [(30, head)], "merged all the same");
    assert_eq!(comments(&rig), [PASSED], "no thanks is posted");
}

#[test]
fn a_head_that_moved_withdraws_the_ruling_and_the_next_pass_raises_another() {
    let (rig, runner, head) = ruled_on();
    rig.claude.script([Scripted::Text(CLEAN)]);
    let moved = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));

    poll(&rig, &runner);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new(), "withdrawn");
    rule(&rig, &runner, "30 yes");
    assert_eq!(rig.forge.merges(), [], "the old head is never merged");

    read_through(&rig, &runner, &moved);
    poll(&rig, &runner);
    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["kind"]["head"], moved);
    assert_ne!(moved, head);
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_no_with_a_note_posts_the_note_in_the_representatives_words() {
    let (rig, runner, head) = ruled_on();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(DECLINE)]);

    rule(&rig, &runner, "30 no Ask for a test before this lands.");
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(
        comments(&rig),
        [PASSED, DECLINE],
        "one post, however many polls"
    );
    let asked = prompts(&rig);
    assert_eq!(asked.len(), 2);
    assert!(
        asked[1].contains("Ask for a test before this lands."),
        "{}",
        asked[1]
    );
    assert_eq!(rig.forge.merges(), []);
    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    // The head is not asked about again, but a new one is.
    assert_eq!(rig.alerts.posts().len(), 1);
    let record = contribution(&rig, &runner);
    assert_eq!(record["head"], head);
    assert_eq!(record["ruled"]["verdict"]["no"], serde_json::json!({}));
}

#[test]
fn a_no_with_no_note_posts_nothing() {
    let (rig, runner, _) = ruled_on();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(DECLINE)]);

    // A reaction or a tick gives the placeholder note.
    rule(&rig, &runner, &format!("30 no {NO_NOTE}"));
    poll(&rig, &runner);
    poll(&rig, &runner);

    assert_eq!(comments(&rig), [PASSED]);
    assert_eq!(prompts(&rig).len(), 1, "only the pass spoke");
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_no_starts_no_fix_turn() {
    let (rig, runner, _) = ruled_on();
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(DECLINE)]);

    rule(&rig, &runner, "30 no Ask for a test before this lands.");
    for _ in 0..4 {
        poll(&rig, &runner);
    }

    let calls = rig.claude.all_seen();
    let roles: Vec<Role> = calls.iter().map(|s| s.call.role).collect();
    assert!(
        roles.iter().all(|r| *r != Role::Worker),
        "no worker turn: {roles:?}"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_items"], serde_json::json!([]), "{status}");
    assert_eq!(status["work_item"], Value::Null);
}

#[test]
fn the_panel_does_not_vote_on_the_merge_ruling() {
    let (rig, runner, _) = ruled_on();
    for _ in 0..3 {
        poll(&rig, &runner);
    }

    let calls = rig.claude.all_seen();
    let panel = [Role::Pm, Role::Advisor, Role::Researcher];
    let asked: Vec<Role> = (calls.iter().map(|s| s.call.role))
        .filter(|r| panel.contains(r))
        .collect();
    assert_eq!(asked, [], "the panel was not called");
    let waiting = rulings(&rig, &runner);
    assert_eq!(waiting.len(), 1, "it waits for the maintainer");
    assert_eq!(waiting[0].get("panel"), None, "{}", waiting[0]);
}

#[test]
fn a_closed_pull_request_drops_the_record_and_the_ruling_with_no_post() {
    let (rig, runner, _) = ruled_on();
    let said = prompts(&rig).len();

    rig.forge.set_state(30, PullRequestState::Closed);
    poll(&rig, &runner);

    assert_eq!(rulings(&rig, &runner), Vec::<Value>::new());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status.get("contributions"), None, "{status}");
    assert_eq!(comments(&rig), [PASSED], "nothing was posted for the close");
    assert_eq!(
        prompts(&rig).len(),
        said,
        "the representative was not asked"
    );
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn the_maintainers_comment_does_not_reset_the_contributors_clock() {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    rig.reviewer
        .script([ScriptedRound::Findings(vec![found()])]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);
    poll(&rig, &runner);
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 30 }));
    let started = contribution(&rig, &runner)["waiting_since"].clone();
    assert_ne!(started, Value::Null, "the findings wait on the contributor");
    assert_eq!(rulings(&rig, &runner).len(), 1, "the merge ruling waits");

    rig.clock.advance(Rig::DAY);
    rig.writes_on(30, "Boss", "Please add a test");
    poll(&rig, &runner);

    let held = contribution(&rig, &runner);
    assert_eq!(held["waiting_since"], started);
    assert_eq!(held["context"].as_array().map(Vec::len), Some(1), "{held}");
    assert_eq!(held.get("heard"), None, "not the contributor's words");
    assert_eq!(rulings(&rig, &runner).len(), 1, "the ruling still waits");
}
