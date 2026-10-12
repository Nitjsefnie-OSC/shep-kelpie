//! A follow-up ruling through the project manager: it reads the findings and
//! the merged diff, and passes the ruling on with a recommendation

use std::sync::Mutex;

use super::{answered, name_pm, pm_seen, step_until};
use crate::ports::AgentError;
use crate::runner::follow_up::tests::{after_merge, auto_merging_on};
use crate::runner::{Runner, StepReport, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted};

const LEAVES_IT: &str = "{\"answer\": null}";

/// A project with a project manager whose issue 7 is ready to merge under
/// `auto`, with `git.issues` as `ask`, the worker having written `work` to
/// `work.txt`
fn ready_to_merge(project: &str, work: &'static str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    name_pm(&rig);
    auto_merging_on(rig, "ask", work)
}

/// ... merged, and the follow-up ruling raised
fn merged_with_a_finding(project: &str, work: &'static str) -> (Rig, Mutex<Runner>) {
    let (rig, runner) = ready_to_merge(project, work);
    let Some(StepReport::Ruling { id: 1, .. }) = after_merge(&runner) else {
        panic!("the maintainer was not asked about the findings");
    };
    (rig, runner)
}

/// What the maintainer was alerted of ruling 1, if anything
fn alerted_of_the_ruling(rig: &Rig) -> Option<String> {
    let posts = rig.alerts.posts();
    let found = posts.iter().find(|(_, a)| a.title.ends_with("ruling 1"));
    found.map(|(_, alert)| alert.text.clone())
}

#[test]
fn a_new_follow_up_ruling_wakes_it_with_the_findings_and_the_merged_diff() {
    let (rig, runner) = merged_with_a_finding("pikachu", "work\n");
    assert_eq!(alerted_of_the_ruling(&rig), None, "the maintainer waits");
    rig.claude.script([Scripted::Say(LEAVES_IT)]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (woke_for, _, _) = answered(Some(report));
    assert_eq!(woke_for.len(), 1, "{woke_for:?}");

    let [wake] = pm_seen(&rig).try_into().unwrap();
    let prompt = &wake.call.prompt;
    assert!(prompt.contains("ruling 1"), "{prompt}");
    assert!(prompt.contains("src/lib.rs:9 looks racy"), "{prompt}");
    assert!(prompt.contains("`diffs/7.diff`"), "{prompt}");
    let diff = std::fs::read_to_string(rig.paths().pm.join("diffs/7.diff")).unwrap();
    assert!(
        diff.contains("+++ b/work.txt\n@@ -0,0 +1 @@\n+work\n"),
        "{diff}"
    );
}

#[test]
fn passing_it_on_leaves_the_ruling_open_with_its_recommendation_and_reason() {
    let (rig, runner) = merged_with_a_finding("bulbasaur", "work\n");
    rig.claude.script([Scripted::Say(
        "{\"answer\": {\"item\": 7, \"escalate\": \"yes\", \"why\": \"the diff shows the race\"}}",
    )]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (_, acted, dropped) = answered(Some(report));
    assert_eq!(
        acted,
        ["put ruling 1 on #7 to the maintainer"],
        "{dropped:?}"
    );
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));

    let text = alerted_of_the_ruling(&rig).expect("the ruling went to the maintainer");
    assert!(
        text.contains("\nPM recommends: yes\nPM says: the diff shows the race"),
        "{text}"
    );
    assert!(text.contains("src/lib.rs:9 looks racy"), "{text}");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["rulings"][0]["kind"]["kind"], "follow-up");
    assert_eq!(rig.forge.created(), [], "nothing is filed before the yes");
}

#[test]
fn text_for_a_follow_up_is_dropped_and_the_ruling_goes_out_as_it_is() {
    let (rig, runner) = merged_with_a_finding("ivysaur", "work\n");
    rig.claude.script([Scripted::Say(
        "{\"answer\": {\"item\": 7, \"text\": \"No.\", \"why\": \"nothing\"}}",
    )]);
    let report = step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));
    let (_, acted, dropped) = answered(Some(report));
    assert_eq!(acted, Vec::<String>::new());
    assert_eq!(dropped.len(), 1, "{dropped:?}");
    assert!(dropped[0].starts_with("answer #7: ruling 1 is a follow-up"));
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let text = alerted_of_the_ruling(&rig).unwrap();
    assert!(!text.contains("PM "), "{text}");
}

#[test]
fn a_wake_that_fails_sends_the_ruling_to_the_maintainer_as_it_is() {
    let (rig, runner) = merged_with_a_finding("squirtle", "work\n");
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude.script([Scripted::Fail(down)]);
    step_until(&runner, |r| matches!(r, StepReport::PmFailed { .. }));
    assert_eq!(step(&runner).unwrap(), Some(StepReport::Alerted { id: 1 }));
    let text = alerted_of_the_ruling(&rig).unwrap();
    assert!(!text.contains("PM recommends"), "{text}");
}

#[test]
fn a_project_manager_that_is_down_lets_the_ruling_go_out_as_it_is() {
    let (rig, runner) = ready_to_merge("charmander", "work\n");
    // A wake that failed takes it out of the running for a while.
    rig.ask(&runner, "tell", Some("Hold #12 until the release."));
    let down = AgentError::Failed(Harness::ClaudeCode, "overloaded".into());
    rig.claude.script([Scripted::Fail(down)]);
    step_until(&runner, |r| matches!(r, StepReport::PmFailed { .. }));

    // Nothing holds the ruling back, so it is alerted as soon as it is raised.
    step_until(&runner, |r| matches!(r, StepReport::Alerted { id: 1 }));
    let text = alerted_of_the_ruling(&rig).expect("the ruling went to the maintainer");
    assert!(text.contains("src/lib.rs:9 looks racy"), "{text}");
    assert_eq!(pm_seen(&rig).len(), 1, "no wake for the ruling");
}

#[test]
fn a_merged_diff_over_64_kib_is_cut() {
    let big = "a line of the merged pull request's work\n".repeat(2_000);
    let (rig, runner) = merged_with_a_finding("meowth", Box::leak(big.into_boxed_str()));
    rig.claude.script([Scripted::Say(LEAVES_IT)]);
    step_until(&runner, |r| matches!(r, StepReport::PmAnswered { .. }));

    let diff = std::fs::read_to_string(rig.paths().pm.join("diffs/7.diff")).unwrap();
    assert!(diff.len() < 65 * 1024, "{} bytes", diff.len());
    let last = diff.trim_end().lines().last().unwrap();
    assert!(
        last.starts_with("[kelpie cut this diff at 64 KiB"),
        "{last}"
    );
}
