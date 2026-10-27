//! The rulings the maintainer puts to the project manager from their thread:
//! what is posted, what is applied, and what is refused

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{name_pm, pm_seen};
use crate::ports::{AgentError, Checks, Role};
use crate::runner::flight::advance;
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::ticket;
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};

/// A project whose worker is parked on merge ruling 71, posted to the
/// maintainer, who may write; with a project manager where `pm` says
fn merge_put(project: &str, pm: bool) -> (Rig, Mutex<Runner>) {
    let (rig, runner, head) = Rig::with_pull_request_set(project, |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        if pm {
            name_pm(rig);
        }
    });
    rig.forge.set_checks(&head, Checks::Passed);
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id, .. }) if id == ticket(71)
    ));
    rig.forge.threads.grant_write("octocat");
    assert!(card(&rig).is_some(), "{:?}", comments(&rig));
    rig.clock.advance(5);
    (rig, runner)
}

/// The project manager's vote on ruling 71 for `choice`, saying `why`
fn vote(choice: char, why: &str) -> &'static str {
    let text = format!(
        "{{\"pick\": null, \"votes\": [{{\"ruling\": 71, \"choice\": \"{choice}\", \"why\": \
         \"{why}\", \"design\": false}}]}}"
    );
    Box::leak(text.into_boxed_str())
}

/// The comments posted on the threads, oldest first
fn comments(rig: &Rig) -> Vec<String> {
    rig.github.comments().into_iter().map(|(_, b)| b).collect()
}

/// The ruling's own comment, the card
fn card(rig: &Rig) -> Option<String> {
    (comments(rig).into_iter()).find(|body| body.starts_with("### Merge this pull request"))
}

/// Writes `reply` as the maintainer, and steps until a comment `wanted`
/// matches is posted, which it returns
fn replied(
    rig: &Rig,
    runner: &Mutex<Runner>,
    reply: &str,
    wanted: impl Fn(&str) -> bool,
) -> String {
    rig.writes_on(71, "octocat", reply);
    rig.next_look();
    posted(rig, runner, wanted)
}

/// Steps until a comment `wanted` matches is posted, which it returns
fn posted(rig: &Rig, runner: &Mutex<Runner>, wanted: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + CALL_PATIENCE;
    loop {
        if let Some(found) = comments(rig).into_iter().find(|c| wanted(c)) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "never posted: {:?}",
            comments(rig)
        );
        step(runner).unwrap();
        std::thread::yield_now();
    }
}

/// Advances the runner, without waiting on a call in flight, until `done`
fn advanced_until(runner: &Mutex<Runner>, done: impl Fn() -> bool) {
    let deadline = Instant::now() + CALL_PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "the runner never got there");
        advance(runner).unwrap();
        std::thread::yield_now();
    }
}

/// Whether ruling 71 still waits
fn pending(rig: &Rig) -> bool {
    let text = std::fs::read_to_string(rig.paths().state).unwrap();
    let state: serde_json::Value = serde_json::from_str(&text).unwrap();
    state["rulings"].as_array().is_some_and(|r| !r.is_empty())
}

/// How many of the project manager's calls carried `lead`
fn woken(rig: &Rig, lead: &str) -> usize {
    (pm_seen(rig).iter())
        .filter(|s| s.call.prompt.contains(lead))
        .count()
}

const RECOMMEND: &str = "asks which option you recommend for ruling 71";

#[test]
fn a_ruling_left_to_the_maintainer_wakes_no_project_manager_until_they_ask() {
    let (rig, runner) = merge_put("asked-quiet", true);
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(vote('A', "PM-WHY"))]);

    for _ in 0..3 {
        step(&runner).unwrap();
    }

    assert_eq!(pm_seen(&rig).len(), 0);
    assert!(!card(&rig).unwrap().contains("**Project manager"));
}

#[test]
fn pm_recommend_posts_its_pick_as_an_existing_answer_and_the_ruling_stays_the_maintainers() {
    let (rig, runner) = merge_put("asked-recommend", true);
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(vote('A', "PM-WHY"))]);

    let said = replied(&rig, &runner, "71 pm recommend", |c| {
        c.contains("**Project manager: A**")
    });

    assert!(
        said.starts_with("@octocat **Project manager: A**. PM-WHY"),
        "{said}"
    );
    assert!(said.contains("It would answer `71 yes` (Merge)."), "{said}");
    assert!(said.contains("Ruling #71 is still yours"), "{said}");
    assert!(pending(&rig), "the maintainer still answers");
    assert_eq!(rig.github.reactions().len(), 1, "kelpie took the request");
    assert_eq!(woken(&rig, RECOMMEND), 1);
}

#[test]
fn its_recommendation_is_inert_text() {
    let (rig, runner) = merge_put("asked-inert", true);
    let why = "ask @mallory, see https://evil.example/x";
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(vote('A', why))]);

    let said = replied(&rig, &runner, "71 pm recommend", |c| {
        c.contains("**Project manager: A**")
    });

    assert!(!said.contains("@mallory"), "{said}");
    assert!(!said.contains("https://"), "{said}");
    assert!(said.contains("@\u{200b}mallory"), "{said}");
}

#[test]
fn with_no_project_manager_a_pm_reply_is_refused_and_says_so() {
    let (rig, runner) = merge_put("asked-none", false);

    let said = replied(&rig, &runner, "71 pm recommend", |c| {
        c.contains("was not put to the project manager")
    });

    assert!(
        said.contains("this project names no project manager"),
        "{said}"
    );
    assert!(rig.github.reactions().is_empty());
}

#[test]
fn a_pm_reply_it_does_not_know_says_what_it_takes() {
    let (rig, runner) = merge_put("asked-unknown", true);

    let said = replied(&rig, &runner, "71 pm whatever", |c| {
        c.contains("was not answered")
    });

    assert!(said.contains("`pm` takes `recommend`"), "{said}");
    assert_eq!(pm_seen(&rig).len(), 0);
}

#[test]
fn a_wake_that_fails_on_a_request_says_so_and_the_ruling_stays_the_maintainers() {
    let (rig, runner) = merge_put("asked-failed", true);
    let failed = AgentError::Failed(Harness::ClaudeCode, "it crashed".into());
    rig.claude.script_for(Role::Pm, [Scripted::Fail(failed)]);

    let said = replied(&rig, &runner, "71 pm recommend", |c| {
        c.contains("did not answer")
    });

    assert_eq!(
        said,
        "@octocat The project manager did not answer what you asked of ruling #71, which is \
         still yours."
    );
    assert!(pending(&rig));
}

#[test]
fn a_second_request_while_its_wake_runs_is_answered_once_and_not_queued() {
    let (rig, runner) = merge_put("asked-twice", true);
    let hold = Hold::default();
    rig.claude.script_for(
        Role::Pm,
        [Scripted::HoldThenSay(hold.clone(), vote('A', "PM-WHY"))],
    );
    rig.writes_on(71, "octocat", "71 pm recommend");
    rig.next_look();
    advanced_until(&runner, || hold.entered(Duration::ZERO));

    rig.clock.advance(5);
    rig.writes_on(71, "octocat", "71 pm recommend");
    rig.next_look();
    let again = || {
        comments(&rig)
            .into_iter()
            .find(|c| c.contains("already on it"))
    };
    advanced_until(&runner, || again().is_some());
    hold.release();
    let said = again().unwrap();
    posted(&rig, &runner, |c| c.contains("**Project manager: A**"));

    assert!(
        said.contains("Ruling #71 was not put to the project manager"),
        "{said}"
    );
    assert_eq!(woken(&rig, RECOMMEND), 1);
    let already = comments(&rig)
        .iter()
        .filter(|c| c.contains("already on it"))
        .count();
    assert_eq!(already, 1);
}
