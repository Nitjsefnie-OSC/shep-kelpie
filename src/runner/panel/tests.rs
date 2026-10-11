//! The panel through the runner's stand-ins: four votes on a merge ruling,
//! what each is shown, and what an expert that fails or gives no vote does

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ports::{AgentError, Role, Tools};
use crate::runner::{Runner, step};
use crate::settings::Harness;
use crate::test::{Rig, Scripted, Seen};

mod fences;
mod folders;

/// How long a test waits on the panel
const PATIENCE: Duration = Duration::from_secs(60);

const WORKER: &str = "Both threads were fixed in the last push, with tests. \
    {\"choice\": \"A\", \"why\": \"WORKER-MARK fixed with tests\", \"design\": false}";
const PM: &str = "{\"pick\": null, \"votes\": [{\"ruling\": 1, \"choice\": \"A\", \
    \"why\": \"PM-MARK CI is green\", \"design\": false}]}";
const ADVISOR: &str = "I read the diff.\n\n{\"choice\": \"B\", \
    \"why\": \"ADVISOR-MARK the hook skips a write\", \"design\": true}";
const RESEARCHER: &str = "Ruling 125 settled it.\n\n{\"choice\": \"A\", \
    \"why\": \"RESEARCHER-MARK precedent\", \"design\": false}";

const MARKS: [&str; 4] = ["WORKER-MARK", "PM-MARK", "ADVISOR-MARK", "RESEARCHER-MARK"];

/// A merge ruling parked on pull request 71, whose project names an
/// advisor and a researcher, and a project manager when `pm`
fn parked(project: &str, pm: bool) -> (Rig, Mutex<Runner>) {
    parked_with(project, pm, |_| {})
}

/// [`parked`], after `setup` changed the rig's settings
fn parked_with(project: &str, pm: bool, setup: impl FnOnce(&Rig)) -> (Rig, Mutex<Runner>) {
    let (rig, runner, _) = Rig::parked_set(project, |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        setup(rig);
        rig.edit_settings(|s| {
            let listed = "\nimplementers = [\"sonnet-high\"]\n";
            assert!(s.contains(listed), "the example's implementers moved");
            let pm = if pm { "pm = \"pm\"\n" } else { "" };
            let named = format!("{listed}advisor = \"advisor\"\nresearcher = \"researcher\"\n{pm}");
            s.replace(listed, &named)
        });
    });
    (rig, runner)
}

/// The ruling's comment on the pull request, once it is posted
fn ruling_comment(rig: &Rig) -> Option<String> {
    (rig.github.comments().into_iter())
        .map(|(_, body)| body)
        .find(|body| body.starts_with("### Merge this pull request"))
}

/// Steps until the ruling's comment is posted, which waits on the panel
fn settle(rig: &Rig, runner: &Mutex<Runner>) -> String {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        step(runner).unwrap();
        if let Some(comment) = ruling_comment(rig) {
            return comment;
        }
        std::thread::yield_now();
    }
    panic!("the ruling's comment was never posted");
}

/// The state file as the runner last saved it
fn state_of(rig: &Rig) -> serde_json::Value {
    let text = std::fs::read_to_string(rig.paths().state).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Every call of `role` the stand-in saw
fn seen(rig: &Rig, role: Role) -> Vec<Seen> {
    (rig.claude.all_seen().into_iter())
        .filter(|s| s.call.role == role)
        .collect()
}

fn script_all(rig: &Rig, pm: bool) {
    rig.claude.script_for(Role::Worker, [Scripted::Say(WORKER)]);
    rig.claude
        .script_for(Role::Advisor, [Scripted::Say(ADVISOR)]);
    rig.claude
        .script_for(Role::Researcher, [Scripted::Say(RESEARCHER)]);
    if pm {
        rig.claude.script_for(Role::Pm, [Scripted::Say(PM)]);
    }
}

#[test]
fn four_votes_fill_the_rulings_panel_in_the_comment_and_in_the_state() {
    let (rig, runner) = parked("koji", true);
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );
    script_all(&rig, true);

    let comment = settle(&rig, &runner);

    assert!(
        comment.contains(
            "**Panel**\n\
             - **Worker** (A): WORKER-MARK fixed with tests\n\
             - **PM** (A): PM-MARK CI is green\n\
             - **Researcher** (A): RESEARCHER-MARK precedent\n\
             - **Advisor** (B): ADVISOR-MARK the hook skips a write\n\n\
             @octocat"
        ),
        "{comment}"
    );
    let state = state_of(&rig);
    let panel = &state["rulings"][0]["panel"];
    assert_eq!(panel.as_array().map(Vec::len), Some(4), "{panel}");
    assert_eq!(
        panel[3]["vote"]["design"], true,
        "the advisor flagged design"
    );
    for role in [Role::Worker, Role::Advisor, Role::Researcher] {
        let calls = seen(&rig, role);
        let votes = calls
            .iter()
            .filter(|s| s.call.prompt.contains("vote on ruling 1"));
        assert_eq!(votes.count(), 1, "{role:?} was asked once");
    }
}

#[test]
fn an_expert_that_gives_no_vote_abstains_and_the_log_says_why() {
    let (rig, runner) = parked("rotom", true);
    rig.claude.script_for(Role::Worker, [Scripted::Say(WORKER)]);
    rig.claude.script_for(
        Role::Advisor,
        [Scripted::Say("It looks fine to me, I would merge it.")],
    );
    rig.claude.script_for(
        Role::Researcher,
        [Scripted::Say(
            "{\"choice\": \"Z\", \"why\": \"RESEARCHER-MARK\", \"design\": false}",
        )],
    );
    let failed = AgentError::Failed(Harness::ClaudeCode, "it crashed".into());
    rig.claude.script_for(Role::Pm, [Scripted::Fail(failed)]);

    let comment = settle(&rig, &runner);

    assert!(
        comment.contains(
            "**Panel**\n\
             - **Worker** (A): WORKER-MARK fixed with tests\n\
             - **PM**: abstained\n\
             - **Researcher**: abstained\n\
             - **Advisor**: abstained\n"
        ),
        "{comment}"
    );
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("ruling 1: the advisor abstains, since its reply held no JSON object"),
        "{notes}"
    );
    assert!(
        notes.contains(
            "ruling 1: the researcher abstains, since `choice` is `Z`, and there are 3 options"
        ),
        "{notes}"
    );
    assert!(notes.contains("ruling 1: the pm abstains"), "{notes}");
}

#[test]
fn no_expert_is_shown_another_experts_vote() {
    let (rig, runner) = parked("pikachu", true);
    script_all(&rig, true);

    settle(&rig, &runner);

    let paths = rig.paths();
    for (voter, own) in ["Worker", "Pm", "Advisor", "Researcher"].iter().zip(MARKS) {
        let role = match *voter {
            "Worker" => Role::Worker,
            "Pm" => Role::Pm,
            "Advisor" => Role::Advisor,
            _ => Role::Researcher,
        };
        for call in seen(&rig, role)
            .iter()
            .filter(|s| s.call.prompt.contains("ruling 1"))
        {
            for other in MARKS.iter().filter(|m| **m != own) {
                assert!(
                    !call.call.prompt.contains(other),
                    "{voter}'s prompt carries {other}: {}",
                    call.call.prompt
                );
            }
        }
    }
    // Nor does a folder kelpie wrote for the two that read one.
    for (folder, own) in [(&paths.advisor, MARKS[2]), (&paths.researcher, MARKS[3])] {
        for text in texts_under(folder) {
            for mark in MARKS.iter().filter(|m| **m != own) {
                assert!(!text.contains(mark), "{} holds {mark}", folder.display());
            }
        }
    }
}

// The text of every file under `dir`
fn texts_under(dir: &Path) -> Vec<String> {
    let mut texts = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match path.is_dir() {
            true => texts.extend(texts_under(&path)),
            false => texts.extend(std::fs::read_to_string(&path)),
        }
    }
    texts
}

#[test]
fn the_worker_abstains_when_its_work_item_has_no_worker_turn() {
    let rig = Rig::new("chelone");
    rig.edit_settings(|s| {
        let listed = "\nimplementers = [\"sonnet-high\"]\n";
        s.replace(listed, &format!("{listed}advisor = \"advisor\"\n"))
    });
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    // The first turn fails before it does anything: a ruling, and no worker turn to resume.
    let failed = AgentError::Failed(Harness::ClaudeCode, "it crashed".into());
    rig.claude.script([Scripted::Fail(failed)]);
    step(&runner).unwrap();
    rig.claude.script_for(
        Role::Advisor,
        [Scripted::Say(
            "{\"choice\": \"A\", \"why\": \"ADVISOR-MARK a retry is cheap\", \"design\": false}",
        )],
    );

    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline && !panel_voted(&rig) {
        step(&runner).unwrap();
    }

    let state = state_of(&rig);
    let panel = state["rulings"][0]["panel"].clone();
    assert_eq!(
        panel,
        serde_json::json!([
            {"voter": "worker", "vote": null},
            {"voter": "advisor", "vote": {"choice": "A", "why": "ADVISOR-MARK a retry is cheap", "design": false}},
        ])
    );
    assert!(
        seen(&rig, Role::Worker)
            .iter()
            .all(|s| !s.call.prompt.contains("vote on ruling")),
        "the worker was never resumed to vote"
    );
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(
        notes.contains("ruling 1: the worker abstains, since #7 has had no worker turn"),
        "{notes}"
    );
}

fn panel_voted(rig: &Rig) -> bool {
    let state = state_of(rig);
    !state["rulings"][0]["panel"].is_null()
}

#[test]
fn the_worker_vote_resumes_its_own_session_and_reads_without_writing() {
    let (rig, runner) = parked("bulbasaur", false);
    script_all(&rig, false);
    settle(&rig, &runner);

    let worker = seen(&rig, Role::Worker);
    let vote = worker
        .iter()
        .find(|s| s.call.prompt.contains("vote on ruling 1"))
        .expect("the worker was asked to vote");
    let first = &worker[0].call;
    assert_eq!(vote.call.session.id(), first.session.id());
    assert!(matches!(
        vote.call.session,
        crate::ports::Session::Resume(_)
    ));
    assert_eq!(vote.call.tools, Tools::Retro);
}

#[test]
fn a_restarted_runner_does_not_ask_the_panel_again_of_a_ruling_that_has_voted() {
    let (rig, runner) = parked("eevee", false);
    script_all(&rig, false);
    settle(&rig, &runner);
    drop(runner);

    let runner = rig.open().unwrap();
    for _ in 0..4 {
        step(&runner).unwrap();
    }

    for role in [Role::Advisor, Role::Researcher] {
        assert_eq!(seen(&rig, role).len(), 1, "{role:?} was asked again");
    }
    let votes = (seen(&rig, Role::Worker).into_iter())
        .filter(|s| s.call.prompt.contains("vote on ruling 1"))
        .count();
    assert_eq!(votes, 1, "the worker was asked again");
}

#[test]
fn a_ruling_queued_behind_another_holds_from_its_turn_not_from_its_take_up() {
    use super::{Desk, HOLD, Open};
    use std::collections::{BTreeMap, BTreeSet};

    let mut desk = Desk::default();
    desk.open.insert(
        2,
        Open {
            since: crate::ports::Timestamp(100),
            waiting: BTreeSet::new(),
            ballots: BTreeMap::new(),
        },
    );
    desk.queue.push_back(2);

    let now = crate::ports::Timestamp(100 + HOLD + 5);
    assert_eq!(desk.queue_pop(now), Some(2));
    assert_eq!(desk.open[&2].since, now);
}

#[test]
fn a_panel_the_settings_dropped_no_longer_holds_its_ruling() {
    let (rig, runner) = parked("jigglypuff", false);
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );
    {
        let mut runner = runner.lock().unwrap();
        runner.agents.advisor = None;
        runner.agents.researcher = None;
    }

    let comment = settle(&rig, &runner);

    assert!(
        comment.starts_with("### Merge this pull request"),
        "{comment}"
    );
}

#[test]
fn a_runner_restarted_while_a_ruling_is_held_posts_its_comment_once_the_panel_has_voted() {
    let (rig, runner) = parked("mew", false);
    assert_eq!(
        ruling_comment(&rig),
        None,
        "the comment waits for the panel"
    );
    assert_eq!(state_of(&rig)["rulings"][0]["unposted"], true);
    drop(runner);
    script_all(&rig, false);

    let runner = rig.open().unwrap();
    let comment = settle(&rig, &runner);

    assert!(comment.contains("**Panel**"), "{comment}");
    assert_eq!(state_of(&rig)["rulings"][0].get("unposted"), None);
}

#[test]
fn a_ruling_whose_work_item_is_dropped_while_the_panel_votes_is_never_posted() {
    let (rig, runner) = parked("snorlax", false);
    script_all(&rig, false);
    rig.ask(&runner, "drop", Some("7"));

    for _ in 0..5 {
        step(&runner).unwrap();
    }

    assert_eq!(ruling_comment(&rig), None);
}
