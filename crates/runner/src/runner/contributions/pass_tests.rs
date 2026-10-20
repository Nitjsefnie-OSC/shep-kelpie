//! The pass of the reviewers over a green contribution head, through the
//! runner's stand-ins

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

use super::budget_tests::settle;
use super::security_tests::{commit_on, contribute, contribution};
use super::tests::poll;
use crate::ports::{
    AgentError, CLEAN, Checks, Cost, Finding, PullRequestState, ReviewerError, Role, Severity,
    Tools,
};
use crate::runner::{CHECKS_SETTLE, Runner, advance, step};
use crate::settings::{Harness, LocalRound};
use crate::test::{
    CALL_PATIENCE, Hold, Rig, Scripted, ScriptedRound, Seen, SeenRound, assert_granted, git,
    installed_sources, source_folders, write_script,
};

// What the security reviewer finds in a head it flags
const FOUND: &str = "HIGH|build.rs:3|downloads and runs a script|it runs at build time";

const HAIKU: &str = "claude-haiku-5-5";

pub(in crate::runner) fn write(folder: &Path, file: &str, text: &str) {
    if let Some(parent) = folder.join(file).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(folder.join(file), text).unwrap();
}

// A project that follows every pull request and lists `list` as its
// contributions' reviewers, over agent files for `mine`, `other` and `docs`,
// stand-in commands (`docs` reads only `docs/`), and `haiku`, a Claude session
pub(in crate::runner) fn listing(list: &[&str]) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer("Boss");
    for name in ["mine", "other", "docs"] {
        let script = rig.home.path().join("bin").join(name);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        write_script(&script, "#!/bin/sh\nexit 0\n");
        let paths = match name {
            "docs" => "paths: [\"docs/**\"]\n",
            _ => "",
        };
        let file = format!(
            "---\nrole: reviewer\nharness: command\ncommand: {}\n{paths}---\n",
            script.display()
        );
        rig.write_agent(name, &file);
    }
    rig.write_agent(
        "haiku",
        "---\nrole: reviewer\nharness: claude-code\nmodel: claude-haiku-5-5\neffort: high\n---\n\
         Read {{DIFF}} for defects.\n",
    );
    rig.edit_settings(|s| {
        format!("{s}\n[app.dogs.kelpie.contributions]\npick = \"all\"\nreviewers = {list:?}\n")
    });
    let runner = rig.open().unwrap();
    (rig, runner)
}

// The local reviewers the stand-in was asked, in order, by their commands' names
pub(in crate::runner) fn asked(rig: &Rig) -> Vec<String> {
    let name = |round: SeenRound| match round.local {
        LocalRound::Command(c) => {
            let file = Path::new(&c.command).file_name().unwrap().to_owned();
            file.to_string_lossy().into_owned()
        }
        LocalRound::Endpoint(_) => "endpoint".to_owned(),
    };
    rig.reviewer.seen().into_iter().map(name).collect()
}

// The sessions `haiku` was asked
fn haiku(rig: &Rig) -> Vec<Seen> {
    let all = rig.claude.all_seen();
    all.into_iter()
        .filter(|s| s.call.role == Role::Reviewer && s.call.model == HAIKU)
        .collect()
}

// Polls until `head` has a pass, and returns the contribution
pub(in crate::runner) fn read_through(rig: &Rig, runner: &Mutex<Runner>, head: &str) -> Value {
    for _ in 0..10 {
        let held = contribution(rig, runner);
        if held.get("pass").is_some_and(|p| p["head"] == head) {
            return held;
        }
        rig.next_look();
        step(runner).unwrap();
        settle(runner);
    }
    panic!("the head was never read: {}", contribution(rig, runner));
}

// Polls three times, enough for a head to be read for security, approved, and
// handed to the reviewers
pub(in crate::runner) fn poll_through(rig: &Rig, runner: &Mutex<Runner>) {
    for _ in 0..3 {
        rig.next_look();
        step(runner).unwrap();
        settle(runner);
    }
}

// Pushes `file` to pull request 30 and reads the new head through
fn push_and_read(rig: &Rig, runner: &Mutex<Runner>, file: &str) -> Value {
    let head = commit_on(rig, 30, "topic/30", |h| write(h, file, "pushed\n"));
    read_through(rig, runner, &head)
}

fn down() -> ScriptedRound {
    ScriptedRound::Fail(ReviewerError::Failed("down".into()))
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

fn names(value: &Value) -> Vec<&str> {
    let all = value.as_array().expect("a list");
    all.iter().map(|n| n.as_str().unwrap()).collect()
}

#[test]
fn the_first_reviewer_to_answer_ends_the_pass() {
    let (rig, runner) = listing(&["mine", "other"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), ["mine"], "the second never ran");
    assert_eq!(held["pass"]["head"], head);
    assert_eq!(names(&held["pass"]["ran"]), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
    assert_eq!(held["pass"].get("findings"), None, "CLEAN is no findings");

    // It is read once: later polls run no one.
    poll(&rig, &runner);
    assert_eq!(asked(&rig), ["mine"]);
}

#[test]
fn the_next_push_starts_at_the_reviewer_after_the_last_that_answered() {
    let (rig, runner) = listing(&["mine", "other", "haiku"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 4));
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);
    assert_eq!(asked(&rig), ["mine"]);

    let second = push_and_read(&rig, &runner, "two.txt");
    assert_eq!(asked(&rig), ["mine", "other"], "it started after mine");
    assert_eq!(second["pass"]["answered"], "other");
    assert_eq!(second["last_answered"], "other");

    let third = push_and_read(&rig, &runner, "three.txt");
    assert_eq!(asked(&rig), ["mine", "other"], "it started after other");
    assert_eq!(haiku(&rig).len(), 1);
    assert_eq!(third["pass"]["answered"], "haiku");
}

#[test]
fn the_start_point_wraps_to_the_front_of_the_list() {
    let (rig, runner) = listing(&["mine", "other"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 2));
    rig.reviewer.script([down(), down(), down()]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    let first = read_through(&rig, &runner, &head);
    assert_eq!(
        first["last_answered"], "other",
        "the last in the list answered"
    );

    let second = push_and_read(&rig, &runner, "two.txt");

    assert_eq!(
        asked(&rig),
        ["mine", "mine", "mine", "other", "mine"],
        "the next pass began again at the front"
    );
    assert_eq!(second["pass"]["answered"], "mine");
}

#[test]
fn a_reviewer_limited_to_paths_the_contribution_does_not_change_is_skipped() {
    let (rig, runner) = listing(&["docs", "mine"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 2));
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), ["mine"], "docs reads only docs/");
    assert_eq!(names(&held["pass"]["ran"]), ["mine"]);

    // A change to docs/ is theirs to read, and the rotation reaches it.
    let held = push_and_read(&rig, &runner, "docs/guide.md");
    assert_eq!(asked(&rig), ["mine", "docs"]);
    assert_eq!(held["pass"]["answered"], "docs");
}

#[test]
fn a_reviewer_that_fails_three_times_is_passed_over_and_the_next_answers() {
    let (rig, runner) = listing(&["mine", "other"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    let answer = ScriptedRound::Findings(vec![found()]);
    rig.reviewer.script([down(), down(), down(), answer]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), ["mine", "mine", "mine", "other"]);
    assert_eq!(names(&held["pass"]["ran"]), ["mine", "other"]);
    assert_eq!(held["pass"]["answered"], "other");
    assert_eq!(held["pass"]["findings"][0]["what"], "leftover placeholder");
}

#[test]
fn when_every_reviewer_fails_the_pass_ends_unread_and_status_says_why() {
    let (rig, runner) = listing(&["mine", "other"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    rig.reviewer.script(std::iter::repeat_with(down).take(6));
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig).len(), 6);
    assert_eq!(held["pass"].get("answered"), None);
    let why = held["pass"]["unread"].as_str().unwrap();
    assert!(why.contains("mine") && why.contains("other"), "{why}");
    assert!(why.contains("kept failing"), "{why}");
    assert_eq!(held.get("last_answered"), None);

    // The head is not read again until it is pushed.
    poll(&rig, &runner);
    assert_eq!(asked(&rig).len(), 6);
}

#[test]
fn a_local_reviewer_that_reviewed_nothing_twice_is_skipped() {
    let (rig, runner) = listing(&["mine", "other"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 4));
    let blank = || {
        let what = "not reviewed: unreadable";
        ScriptedRound::Findings(vec![Finding::new(Severity::Low, "one.txt", 0, what, "")])
    };
    let clean = ScriptedRound::Findings(Vec::new());
    rig.reviewer.script([blank(), clean, blank()]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);
    push_and_read(&rig, &runner, "two.txt");
    assert_eq!(asked(&rig), ["mine", "other", "mine", "other"]);

    let third = push_and_read(&rig, &runner, "three.txt");

    assert_eq!(
        asked(&rig),
        ["mine", "other", "mine", "other", "other"],
        "mine reviewed nothing twice"
    );
    assert_eq!(third["pass"]["answered"], "other");

    // It sat that pass out, and is tried again at the next.
    push_and_read(&rig, &runner, "four.txt");
    assert_eq!(asked(&rig).last().unwrap(), "mine");
}

#[test]
fn no_reviewer_reads_a_head_whose_ci_is_not_green() {
    let (rig, runner) = listing(&["mine"]);
    rig.ci_on_now(&runner);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 2));
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    poll_through(&rig, &runner);
    let held = contribution(&rig, &runner);
    assert_eq!(held["passed"], head);
    assert_eq!(held["ci"]["state"], "waiting");
    assert_eq!(held.get("pass"), None, "{held}");
    assert_eq!(asked(&rig), Vec::<String>::new());

    let red = Checks::Failed(vec!["test".into()]);
    rig.forge.set_checks(&head, red);
    rig.clock.advance(CHECKS_SETTLE);
    poll_through(&rig, &runner);
    assert_eq!(contribution(&rig, &runner)["ci"]["state"]["red"][0], "test");
    assert_eq!(contribution(&rig, &runner).get("pass"), None);
    assert_eq!(asked(&rig), Vec::<String>::new(), "red is not green");

    // A push that goes green is read.
    let next = commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    rig.forge.set_checks(&next, Checks::Passed);
    poll_through(&rig, &runner);
    rig.clock.advance(CHECKS_SETTLE);
    let held = read_through(&rig, &runner, &next);
    assert_eq!(held["pass"]["head"], next);
    assert_eq!(asked(&rig), ["mine"]);
}

#[test]
fn no_reviewer_reads_a_head_that_has_not_passed_security() {
    let (rig, runner) = listing(&["mine", "haiku"]);
    rig.claude.script([Scripted::Text(FOUND)]);
    contribute(&rig, 30, |h| write(h, "build.rs", "fn main() {}\n"));

    poll_through(&rig, &runner);

    let held = contribution(&rig, &runner);
    assert_eq!(held.get("passed"), None, "{held}");
    assert_eq!(held.get("pass"), None, "{held}");
    assert_eq!(asked(&rig), Vec::<String>::new());
    assert_eq!(haiku(&rig).len(), 0);
}

#[test]
fn the_reviewers_call_is_read_only_and_runs_in_the_fetched_worktree() {
    let (rig, runner) = listing(&["haiku", "mine"]);
    rig.claude
        .script(std::iter::repeat_n(Scripted::Text(CLEAN), 3));
    let first = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    let folder: PathBuf = rig.paths().folder.join("reading").join("30");

    read_through(&rig, &runner, &first);

    let calls = haiku(&rig);
    assert_eq!(calls.len(), 1);
    let call = &calls[0].call;
    assert_eq!(call.tools, Tools::Review, "it reads and runs nothing");
    assert_eq!(call.cwd, folder);
    assert_eq!(git(&folder, &["rev-parse", "HEAD"]), first);
    assert_eq!(
        std::fs::read_to_string(folder.join("one.txt")).unwrap(),
        "one\n"
    );

    // The local reviewer is given the same folder, at the new head.
    let second = push_and_read(&rig, &runner, "two.txt");
    let round = &rig.reviewer.seen()[0];
    assert_eq!(round.worktree, folder);
    assert_eq!(round.base, "origin/main");
    assert_eq!(second["pass"]["answered"], "mine");
    assert_eq!(
        git(&folder, &["rev-parse", "HEAD"]),
        second["head"].as_str().unwrap()
    );
    assert!(folder.join("two.txt").exists());
}

#[test]
fn a_head_that_changes_agents_own_files_is_read_by_no_one() {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, ".claude/settings.json", "{}\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(asked(&rig), Vec::<String>::new());
    let why = held["pass"]["unread"].as_str().unwrap();
    assert!(why.contains(".claude/settings.json"), "{why}");
}

#[test]
fn findings_are_recorded_on_the_contribution() {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    rig.reviewer
        .script([ScriptedRound::Findings(vec![found()])]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    let finding = &held["pass"]["findings"][0];
    assert_eq!(finding["file"], "one.txt");
    assert_eq!(finding["line"], 1);
    assert_eq!(finding["what"], "leftover placeholder");
    assert_eq!(finding["why"], "it ships to users");
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn the_worktree_is_removed_when_the_pull_request_leaves_the_record() {
    let (rig, runner) = listing(&["mine"]);
    rig.claude.script([Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    let folder = rig.paths().folder.join("reading").join("30");
    read_through(&rig, &runner, &head);
    assert!(folder.join("one.txt").exists());

    rig.forge.set_state(30, PullRequestState::Closed);
    poll(&rig, &runner);

    assert!(!folder.exists(), "the checkout is gone");
}

#[test]
fn a_reviewer_session_that_fails_is_tried_three_times_then_passed_over() {
    let (rig, runner) = listing(&["haiku", "mine"]);
    let fail = || Scripted::Fail(AgentError::Failed(Harness::ClaudeCode, "overloaded".into()));
    rig.claude
        .script([Scripted::Text(CLEAN), fail(), fail(), fail()]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    let held = read_through(&rig, &runner, &head);

    assert_eq!(haiku(&rig).len(), 3);
    assert_eq!(asked(&rig), ["mine"]);
    assert_eq!(held["pass"]["answered"], "mine");
}

#[test]
fn a_pass_past_its_ceiling_ends_unread_and_counts_what_it_spent() {
    let (rig, runner) = listing(&["haiku", "mine"]);
    let hold = Hold::default();
    rig.claude.script([
        Scripted::Text(CLEAN),
        Scripted::Billed("no verdict", Cost(5 * Cost::PER_CENT)),
        Scripted::Hold(hold.clone()),
    ]);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    for _ in 0..10 {
        if hold.entered(std::time::Duration::from_millis(50)) {
            break;
        }
        rig.next_look();
        advance(&runner).unwrap();
    }
    assert!(hold.entered(CALL_PATIENCE), "the pass never began");

    rig.clock.advance(16 * 60);
    advance(&runner).unwrap();
    settle(&runner);

    let held = contribution(&rig, &runner);
    assert_eq!(held["spend"], 5 * Cost::PER_CENT);
    let why = held["pass"]["unread"].as_str().unwrap();
    assert!(why.contains("haiku") && why.contains("ceiling"), "{why}");
    assert_eq!(held["pass"]["ran"], json!(["haiku"]));
    assert_eq!(
        asked(&rig),
        Vec::<String>::new(),
        "no reviewer gets another turn"
    );
}

#[test]
fn a_pass_reviewer_reads_the_installed_dependency_sources_and_no_credential_of_them() {
    let (rig, runner) = listing(&["haiku"]);
    let (cargo, rustup, secret) = installed_sources(&rig);
    rig.claude
        .script([Scripted::Text(CLEAN), Scripted::Text(CLEAN)]);
    let head = contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    read_through(&rig, &runner, &head);

    let [seen] = haiku(&rig).try_into().unwrap();
    assert_granted(&seen, &source_folders(&cargo, &rustup), &secret);
}
