//! What kelpie tells a contributor, and what it asks, once their pull request is its own
//!
//! Through the runner's stand-ins: a released fork contribution adopted by
//! the runner, with the representative's stand-in saying what is scripted.

use std::path::Path;
use std::sync::Mutex;

use crate::ports::{AgentError, CLEAN, Checks, Role};
use crate::runner::adopt::released::{FORK_BRANCH, adopted, contributed, release};
use crate::runner::{Runner, StepReport, step};
use crate::state::RulingKind;
use crate::test::{Rig, Scripted, git, ticket};

const ON_IT: &str = "CI failed on build. Kelpie is fixing it on your branch.";
const AGAIN: &str = "CI failed again on lint. Kelpie is fixing that too.";

// Pull request 80, released and adopted from a fork that allows edits.
// Returns the fork's URL.
fn adopted_fork(said: impl IntoIterator<Item = Scripted>) -> (Rig, Mutex<Runner>, String) {
    let (rig, runner) = contributed();
    rig.ci_on_now(&runner);
    let (url, _) = rig.push_to_fork(FORK_BRANCH, "fork.txt");
    rig.forge.set_fork(80, &url, true);
    release(&runner);
    assert_eq!(rig.ask(&runner, "adopt", Some("80")).get("error"), None);
    assert_eq!(step(&runner).unwrap(), Some(adopted()));
    rig.claude.script_for(Role::Representative, said);
    (rig, runner, url)
}

fn failed(check: &str) -> Checks {
    Checks::Failed(vec![check.to_owned()])
}

fn on_fork(url: &str) -> String {
    git(
        Path::new(url),
        &["rev-parse", &format!("refs/heads/{FORK_BRANCH}")],
    )
}

// Steps until red CI on the fork's head, whichever head the catch-up with
// `main` leaves, is the worker's next turn. Returns the head it failed at.
fn until_red(rig: &Rig, runner: &Mutex<Runner>, url: &str, check: &str) -> String {
    for _ in 0..12 {
        rig.forge.set_checks(&on_fork(url), failed(check));
        if let Some(StepReport::CiFailed { head, .. }) = rig.verdict(runner) {
            return head;
        }
    }
    panic!("CI never went to the worker");
}

// What the App posted on the pull requests
fn comments(rig: &Rig) -> Vec<String> {
    (rig.github.comments().into_iter())
        .map(|(_, body)| body)
        .collect()
}

// The prompts of the representative's calls
fn prompts(rig: &Rig) -> Vec<String> {
    (rig.claude.all_seen().into_iter())
        .filter(|s| s.call.role == Role::Representative)
        .map(|s| s.call.prompt)
        .collect()
}

fn worktree_head(rig: &Rig) -> String {
    git(&rig.paths().worktree(ticket(5)), &["rev-parse", "HEAD"])
}

#[test]
fn a_ci_fix_turn_on_an_adopted_contribution_posts_one_comment_saying_kelpie_is_on_it() {
    let (rig, runner, url) = adopted_fork([Scripted::Say(ON_IT)]);

    let head = until_red(&rig, &runner, &url, "build");

    assert_eq!(head, on_fork(&url));
    assert_eq!(comments(&rig), [ON_IT]);
    let [prompt] = prompts(&rig).try_into().unwrap();
    assert!(
        prompt.contains("build"),
        "the failed check is a fact: {prompt}"
    );
    assert!(
        prompt.contains("Never promise or hint at a merge"),
        "no promise of a merge: {prompt}"
    );
}

#[test]
fn a_second_fix_turn_posts_a_second_comment() {
    let (rig, runner, url) = adopted_fork([Scripted::Say(ON_IT), Scripted::Say(AGAIN)]);
    let first = until_red(&rig, &runner, &url, "build");
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    assert_ne!(on_fork(&url), first, "the fix was pushed");

    let second = until_red(&rig, &runner, &url, "lint");

    assert_ne!(second, first);
    assert_eq!(comments(&rig), [ON_IT, AGAIN]);
}

#[test]
fn a_failed_representative_call_does_not_hold_the_fix_turn_back() {
    let (rig, runner, url) = adopted_fork([Scripted::Fail(AgentError::Setup("no".into()))]);

    let head = until_red(&rig, &runner, &url, "build");

    assert_eq!(comments(&rig), Vec::<String>::new(), "nothing was posted");
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    assert_ne!(
        on_fork(&url),
        head,
        "the worker's fix turn ran and was pushed"
    );
    assert_eq!(
        comments(&rig),
        Vec::<String>::new(),
        "and nothing is retried"
    );
}

// Pull request 80 adopted; the contributor pushes before the gate looks.
// Steps until the foreign change is ruled on.
fn pushed_after_release() -> (Rig, Mutex<Runner>, String) {
    let (rig, runner, url) = adopted_fork([]);
    // The project's reviewers read an adopted pull request before CI does.
    rig.claude.script([Scripted::Text(CLEAN)]);
    let (_, theirs) = rig.push_to_fork(FORK_BRANCH, "theirs.txt");
    assert_eq!(on_fork(&url), theirs);
    rig.forge.set_checks(&theirs, Checks::Passed);
    let ruled = (0..12).any(|_| {
        rig.verdict(&runner);
        !foreign(&runner).is_empty()
    });
    assert!(ruled, "the contributor's push was ruled on");
    (rig, runner, theirs)
}

fn foreign(runner: &Mutex<Runner>) -> Vec<RulingKind> {
    let runner = runner.lock().unwrap();
    (runner.state.rulings.iter())
        .filter(|r| r.pull_request == Some(80) && !r.kind.is_contribution())
        .map(|r| r.kind.clone())
        .filter(|kind| matches!(kind, RulingKind::ForeignChange { .. }))
        .collect()
}

#[test]
fn a_contributors_push_after_release_parks_foreign_change() {
    let (rig, runner, theirs) = pushed_after_release();

    let kinds = foreign(&runner);
    let [RulingKind::ForeignChange { known, .. }] = kinds.as_slice() else {
        panic!("one foreign change was due: {kinds:?}");
    };
    assert_eq!(known.head.as_deref(), Some(theirs.as_str()));
    assert_ne!(
        worktree_head(&rig),
        theirs,
        "the worker's worktree is not moved under it"
    );
}

#[test]
fn a_yes_to_that_ruling_adopts_the_new_head() {
    let (rig, runner, theirs) = pushed_after_release();

    let answered = rig.ask(&runner, "rule", Some("80 yes"));

    assert_eq!(answered.get("error"), None, "{answered}");
    assert_eq!(foreign(&runner), [], "the ruling is answered");
    assert_eq!(worktree_head(&rig), theirs);
    let runner = runner.lock().unwrap();
    assert_eq!(
        runner.current().unwrap().known.head.as_deref(),
        Some(theirs.as_str())
    );
}

#[test]
fn an_ordinary_kelpie_pull_requests_red_ci_posts_no_comment() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    rig.forge.set_checks(&head, failed("build"));
    rig.claude
        .script_for(Role::Representative, [Scripted::Say(ON_IT)]);

    let report = rig.verdict(&runner);

    assert!(
        matches!(report, Some(StepReport::CiFailed { .. })),
        "{report:?}"
    );
    assert_eq!(comments(&rig), Vec::<String>::new());
    assert_eq!(prompts(&rig), Vec::<String>::new());
}
