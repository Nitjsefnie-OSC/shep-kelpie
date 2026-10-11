//! The panel deciding through the runner's stand-ins: three votes of four,
//! the smaller panels a work item's type sets, and the project manager alone

use std::sync::Mutex;
use std::time::Instant;

use super::{PATIENCE, parked_with, seen, state_of};
use crate::ports::{Checks, Role};
use crate::runner::{Runner, StepReport, step};
use crate::settings::{Decide, Merging, RulingName};
use crate::test::{Rig, Scripted};

mod measures;
mod reopening;

/// An expert's reply voting `choice` with a reason that names it
fn says(who: &str, choice: char, design: bool) -> String {
    format!("{{\"choice\": \"{choice}\", \"why\": \"{who}-WHY\", \"design\": {design}}}")
}

/// The project manager's reply voting `choice` on ruling 1
fn pm_says(choice: char, design: bool) -> String {
    format!(
        "{{\"pick\": null, \"votes\": [{{\"ruling\": 1, \"choice\": \"{choice}\", \
         \"why\": \"PM-WHY\", \"design\": {design}}}]}}"
    )
}

/// What the four vote: the worker, the project manager, the researcher and
/// the advisor, a letter each, a `!` after it flagging `design`
fn script(rig: &Rig, votes: [&str; 4]) {
    let pm = pm_says(votes[1].chars().next().unwrap(), votes[1].ends_with('!'));
    script_with_pm(rig, votes, pm);
}

/// [`script`], with the project manager replying `pm_reply` in place of its vote
fn script_with_pm(rig: &Rig, votes: [&str; 4], pm_reply: String) {
    let (worker, researcher, advisor) = (votes[0], votes[2], votes[3]);
    let reply = |who: &str, vote: &str| {
        let design = vote.ends_with('!');
        let choice = vote.chars().next().unwrap();
        says(who, choice, design)
    };
    rig.claude
        .script_for(Role::Worker, [Scripted::Say(leak(reply("WORKER", worker)))]);
    rig.claude.script_for(
        Role::Researcher,
        [Scripted::Say(leak(reply("RESEARCHER", researcher)))],
    );
    rig.claude.script_for(
        Role::Advisor,
        [Scripted::Say(leak(reply("ADVISOR", advisor)))],
    );
    rig.claude
        .script_for(Role::Pm, [Scripted::Say(leak(pm_reply))]);
}

// A scripted reply lives as long as the test binary does.
fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A merge ruling parked on pull request 71, whose pull request is titled
/// `title`, under `merging`, with `rulings` as the `[rulings]` table
fn ruling(project: &str, title: &str, merging: Merging, rulings: &str) -> (Rig, Mutex<Runner>) {
    let table = format!("\n[app.dogs.kelpie.rulings]\n{rulings}\n");
    let (rig, runner) = parked_with(project, true, |rig| {
        rig.forge.set_title(71, title);
        rig.edit_settings(|s| format!("{s}{table}"));
    });
    // The merge ruling was raised under `ask`, and is judged under `merging`.
    runner.lock().unwrap().settings.git.merging = merging;
    (rig, runner)
}

/// Steps until `done` holds of the state file, or the wait runs out
fn until(rig: &Rig, runner: &Mutex<Runner>, done: impl Fn(&serde_json::Value) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        step(runner).unwrap();
        if done(&state_of(rig)) {
            return;
        }
        std::thread::yield_now();
    }
    panic!("the state never came to be: {}", state_of(rig));
}

/// Steps until the panel has voted on ruling 1, whether it decided or not
fn voted(rig: &Rig, runner: &Mutex<Runner>) {
    let (rig_ref, deadline) = (rig, Instant::now() + PATIENCE);
    while Instant::now() < deadline {
        step(runner).unwrap();
        let state = state_of(rig_ref);
        let gone = state["rulings"].as_array().is_some_and(Vec::is_empty);
        let voted = !state["rulings"][0]["panel"].is_null();
        if gone || voted {
            // One pass more, for the comment a ruling that waits is posted with.
            step(runner).unwrap();
            return;
        }
        std::thread::yield_now();
    }
    panic!("the panel never voted: {}", state_of(rig));
}

/// The comments posted on the pull request, oldest first
fn comments(rig: &Rig) -> Vec<String> {
    rig.github.comments().into_iter().map(|(_, b)| b).collect()
}

/// The comment that says the panel decided, if one was posted
fn decided_comment(rig: &Rig) -> Option<String> {
    comments(rig)
        .into_iter()
        .find(|body| body.starts_with("**Decided by the panel"))
}

/// The comment that asks the maintainer, if one was posted
fn asking_comment(rig: &Rig) -> Option<String> {
    comments(rig)
        .into_iter()
        .find(|body| body.starts_with("### Merge this pull request"))
}

/// Whether ruling 1 still waits
fn pending(rig: &Rig) -> bool {
    state_of(rig)["rulings"]
        .as_array()
        .is_some_and(|r| !r.is_empty())
}

/// How many times `role` was asked to vote on ruling 1
fn asked(rig: &Rig, role: Role) -> usize {
    (seen(rig, role).iter())
        .filter(|s| s.call.prompt.contains("vote on ruling 1"))
        .count()
}

const PANEL: &str = "decide = \"panel\"";
const SHADOW: &str = "decide = \"shadow\"";

#[test]
fn four_votes_for_one_option_decide_it_under_panel() {
    let (rig, runner) = ruling("a4", "feat: add it", Merging::Auto, PANEL);
    script(&rig, ["A", "A", "A", "A"]);

    voted(&rig, &runner);

    assert!(!pending(&rig), "the panel applied its decision");
    let said = decided_comment(&rig).expect("the decision is posted");
    assert!(
        said.contains("Decided by the panel: A, Merge** (4 of 4)"),
        "{said}"
    );
    assert!(said.contains("- **Advisor** (A): ADVISOR-WHY"), "{said}");
    assert_eq!(asking_comment(&rig), None, "the maintainer was never asked");
}

#[test]
fn three_votes_of_four_decide_it_under_panel() {
    let (rig, runner) = ruling("b3", "feat: add it", Merging::Auto, PANEL);
    script(&rig, ["A", "A", "B", "A"]);

    voted(&rig, &runner);

    assert!(!pending(&rig));
    let said = decided_comment(&rig).expect("the decision is posted");
    assert!(said.contains("(3 of 4)"), "{said}");
    assert!(
        said.contains("- **Researcher** (B): RESEARCHER-WHY"),
        "{said}"
    );
}

#[test]
fn under_shadow_the_same_votes_are_posted_with_a_recommendation_and_decide_nothing() {
    let (rig, runner) = ruling("c-shadow", "feat: add it", Merging::Auto, SHADOW);
    script(&rig, ["A", "A", "A", "A"]);

    voted(&rig, &runner);

    assert!(pending(&rig), "the maintainer still answers");
    assert_eq!(decided_comment(&rig), None);
    let asking = asking_comment(&rig).expect("the ruling is posted for the maintainer");
    assert!(
        asking.contains("**Recommendation: A** (4 of 4)"),
        "{asking}"
    );
}

#[test]
fn a_split_goes_to_the_maintainer_with_every_vote_and_no_recommendation() {
    let (rig, runner) = ruling("d-split", "feat: add it", Merging::Auto, PANEL);
    script(&rig, ["A", "B", "A", "B"]);

    voted(&rig, &runner);

    assert!(pending(&rig));
    let asking = asking_comment(&rig).expect("the ruling is posted for the maintainer");
    for who in [
        "Worker** (A)",
        "PM** (B)",
        "Researcher** (A)",
        "Advisor** (B)",
    ] {
        assert!(asking.contains(who), "{who}: {asking}");
    }
    assert!(!asking.contains("Recommendation"), "{asking}");
}

#[test]
fn a_design_flag_goes_to_the_maintainer_with_the_recommendation() {
    let (rig, runner) = ruling("e-design", "feat: add it", Merging::Auto, PANEL);
    script(&rig, ["A", "A", "A", "A!"]);

    voted(&rig, &runner);

    assert!(pending(&rig));
    assert_eq!(decided_comment(&rig), None);
    let asking = asking_comment(&rig).expect("the ruling is posted for the maintainer");
    assert!(
        asking.contains("**Recommendation: A** (4 of 4)"),
        "{asking}"
    );
}

#[test]
fn a_merge_under_ask_goes_to_the_maintainer_at_four_to_nothing() {
    let (rig, runner) = ruling("f-ask", "feat: add it", Merging::Ask, PANEL);
    script(&rig, ["A", "A", "A", "A"]);

    voted(&rig, &runner);

    assert!(pending(&rig));
    assert_eq!(decided_comment(&rig), None);
    let asking = asking_comment(&rig).expect("the ruling is posted for the maintainer");
    assert!(
        asking.contains("**Recommendation: A** (4 of 4)"),
        "{asking}"
    );
}

#[test]
fn a_docs_item_calls_only_the_project_manager() {
    let (rig, runner) = ruling("g-docs", "docs(readme): say more", Merging::Auto, PANEL);
    script(&rig, ["B", "A", "B", "B"]);

    until(&rig, &runner, |state| {
        state["rulings"].as_array().is_some_and(Vec::is_empty)
    });

    assert_eq!(asked(&rig, Role::Advisor), 0);
    assert_eq!(asked(&rig, Role::Researcher), 0);
    assert_eq!(asked(&rig, Role::Worker), 0);
    let said = decided_comment(&rig).expect("the project manager's vote decided it");
    assert!(said.contains("(1 of 1)"), "{said}");
}

#[test]
fn a_fix_item_calls_the_project_manager_and_the_advisor_only() {
    let (rig, runner) = ruling("h-fix", "fix(guard): a bug", Merging::Auto, PANEL);
    script(&rig, ["B", "A", "B", "A"]);

    until(&rig, &runner, |state| {
        state["rulings"].as_array().is_some_and(Vec::is_empty)
    });

    assert_eq!(asked(&rig, Role::Advisor), 1);
    assert_eq!(asked(&rig, Role::Researcher), 0);
    assert_eq!(asked(&rig, Role::Worker), 0);
    let said = decided_comment(&rig).expect("both agreeing decided it");
    assert!(said.contains("(2 of 2)"), "{said}");
}

#[test]
fn a_split_in_a_smaller_panel_asks_the_next_one_up() {
    let (rig, runner) = ruling("i-up", "fix: a bug", Merging::Auto, PANEL);
    // The advisor disagrees with the project manager, so the other two are asked.
    script(&rig, ["A", "A", "A", "B"]);

    until(&rig, &runner, |state| {
        state["rulings"].as_array().is_some_and(Vec::is_empty)
    });

    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
    let said = decided_comment(&rig).expect("three of four decided it");
    assert!(said.contains("(3 of 4)"), "{said}");
}

#[test]
fn a_design_flag_in_a_smaller_panel_asks_the_next_one_up() {
    let (rig, runner) = ruling("j-docs-design", "docs: say more", Merging::Auto, PANEL);
    // The project manager flags design, so the project manager and the advisor are asked,
    // and then all four.
    script(&rig, ["A", "A!", "A", "A"]);

    voted(&rig, &runner);

    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 1, "{role:?}");
    }
    assert!(pending(&rig), "the flag stands, so the maintainer decides");
    let asking = asking_comment(&rig).expect("the ruling is posted for the maintainer");
    assert!(
        asking.contains("**Recommendation: A** (4 of 4)"),
        "{asking}"
    );
}

/// A merge ruling parked on pull request 71 as [`ruling`] has it, but under
/// `panel` with `merge` on `rulings.pm_alone` from the start. The runner
/// refuses that list when it opens, so it is set once it has.
fn alone(project: &str, title: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new(project);
    rig.with_app();
    rig.maintainer("octocat");
    super::name_panel(&rig, true);
    rig.forge.set_title(71, title);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::Push("work.txt", "work\n"),
        Scripted::Text("CLEAN"),
    ]);
    step(&runner).unwrap(); // the worker\'s first turn: opens the pull request
    step(&runner).unwrap(); // review round 1
    step(&runner).unwrap(); // review round 2
    let head = rig.forge.head_of("kelpie/7").expect("the worker pushed");
    rig.ci_on_now(&runner);
    // Turning CI on read the settings again, so the list is set after it.
    {
        let mut runner = runner.lock().unwrap();
        runner.settings.rulings.decide = Decide::Panel;
        runner.settings.rulings.pm_alone = vec![RulingName::Merge];
    }
    rig.forge.set_checks(&head, Checks::Passed);
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id: 1, .. })
    ));
    // Under `ask` it was raised; it is judged under `auto`.
    runner.lock().unwrap().settings.git.merging = Merging::Auto;
    (rig, runner)
}

#[test]
fn a_kind_on_pm_alone_is_decided_by_the_project_manager_with_no_other_call() {
    let (rig, runner) = alone("k-alone", "feat: add it");
    script(&rig, ["B", "A", "B", "B"]);

    until(&rig, &runner, |state| {
        state["rulings"].as_array().is_some_and(Vec::is_empty)
    });

    for role in [Role::Advisor, Role::Researcher, Role::Worker] {
        assert_eq!(asked(&rig, role), 0, "{role:?}");
    }
    let said = decided_comment(&rig).expect("the project manager decided it alone");
    assert!(said.contains("(1 of 1)"), "{said}");
}

#[test]
fn a_design_flag_or_an_abstention_on_pm_alone_calls_the_types_panel() {
    for (project, vote) in [
        ("l-design", pm_says('A', true)),
        ("m-abstain", "no vote".into()),
    ] {
        let (rig, runner) = alone(project, "feat: add it");
        script_with_pm(&rig, ["A", "A", "A", "A"], vote);

        voted(&rig, &runner);

        // A `feat` item\'s panel is all four, so they were each asked once.
        for role in [Role::Advisor, Role::Researcher, Role::Worker] {
            assert_eq!(asked(&rig, role), 1, "{project}: {role:?}");
        }
    }
}

#[test]
fn a_merge_kind_on_pm_alone_stops_the_runner_naming_the_setting() {
    let rig = Rig::new("n-refused");
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.rulings]\npm_alone = [\"merge\"]\n"));

    let err = match rig.open() {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the runner opened"),
    };

    assert!(err.contains("rulings.pm_alone"), "{err}");
    assert!(err.contains("merge"), "{err}");
}
