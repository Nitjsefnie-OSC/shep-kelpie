//! The worker's commits on a contributor's fork, pushed by kelpie
//!
//! A worker on a fork only commits (the guard refuses its `git push`, see
//! `crates/core/src/guard/tests/fork.rs`). After each of its turns the runner
//! pushes the commits to the contributor's branch, fast-forward only.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::ports::{CLEAN, Checks, Finding, PullRequestState, Severity};
use crate::runner::adopt::released::{FORK_BRANCH, adopted, contributed, release};
use crate::runner::contributions::Sent;
use crate::runner::git_reads::Found;
use crate::runner::ruling::Answer;
use crate::runner::{Runner, StepReport, step};
use crate::state::{RulingKind, Stuck};
use crate::test::{Hold, Rig, Scripted, ScriptedRound, git, ticket};

// Pull request 80, released and adopted from a fork that allows edits, read by
// the reviewers and awaiting the maintainer's merge ruling, which a no answers
// with a note: the next step is the worker's turn. Returns the fork's URL and
// the head kelpie adopted.
fn noted() -> (Rig, Mutex<Runner>, String, String) {
    let (rig, runner) = contributed();
    let (url, head) = rig.push_to_fork(FORK_BRANCH, "fork.txt");
    rig.forge.set_fork(80, &url, true);
    release(&runner);
    assert_eq!(rig.ask(&runner, "adopt", Some("80")).get("error"), None);
    assert_eq!(step(&runner).unwrap(), Some(adopted()));
    rig.forge.set_checks(&head, Checks::Passed);
    // The project's reviewers read an adopted pull request before CI does.
    rig.claude.script([Scripted::Text(CLEAN)]);
    let ruled = (0..12).any(|_| matches!(rig.verdict(&runner), Some(StepReport::Ruling { .. })));
    assert!(ruled, "the merge ruling was raised");
    let note = Answer::No("handle the empty case".to_owned());
    runner.lock().unwrap().rule(ticket(80), note).unwrap();
    (rig, runner, url, head)
}

fn on_fork(url: &str) -> String {
    git(
        Path::new(url),
        &["rev-parse", &format!("refs/heads/{FORK_BRANCH}")],
    )
}

fn worktree_head(rig: &Rig) -> String {
    git(&rig.paths().worktree(ticket(5)), &["rev-parse", "HEAD"])
}

fn raised(runner: &Mutex<Runner>) -> Vec<RulingKind> {
    let runner = runner.lock().unwrap();
    (runner.state.rulings.iter())
        .filter(|r| r.pull_request == Some(80) && !r.kind.is_contribution())
        .map(|r| r.kind.clone())
        .collect()
}

#[test]
fn the_runner_pushes_the_workers_commits_to_the_forks_branch_after_a_turn() {
    let (rig, runner, url, head) = noted();
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    step(&runner).unwrap();

    let committed = worktree_head(&rig);
    assert_ne!(committed, head, "the worker committed");
    assert_eq!(on_fork(&url), committed, "kelpie pushed it");
    assert_eq!(
        git(Path::new(&url), &["rev-parse", &format!("{committed}~1")]),
        head,
        "on top of the contributor's head"
    );
    assert_eq!(raised(&runner), [], "nothing is a foreign change");
    assert_eq!(
        rig.forge.head_of("kelpie/5"),
        None,
        "nothing reached origin"
    );
}

#[test]
fn a_fork_head_that_moved_raises_foreign_change_and_the_commits_are_not_pushed() {
    let (rig, runner, url, head) = noted();
    // The contributor pushes while the worker is at work.
    let (_, theirs) = rig.push_to_fork(FORK_BRANCH, "theirs.txt");
    assert_ne!(theirs, head);
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    step(&runner).unwrap();
    rig.verdict(&runner);

    assert_eq!(on_fork(&url), theirs, "their push stands");
    assert_ne!(worktree_head(&rig), theirs, "the commits wait, unpushed");
    let kinds = raised(&runner);
    let [RulingKind::ForeignChange { known, .. }] = kinds.as_slice() else {
        panic!("a foreign change was due: {kinds:?}");
    };
    assert_eq!(known.head.as_deref(), Some(theirs.as_str()));
}

#[test]
fn a_refused_push_raises_a_ruling_naming_the_reason_and_keeps_the_commits() {
    let (rig, runner, url, head) = noted();
    // Edits are turned off: the fork refuses what is pushed to it.
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'edits by maintainers are off' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    step(&runner).unwrap();
    rig.verdict(&runner);

    assert_eq!(on_fork(&url), head, "nothing was pushed");
    let kinds = raised(&runner);
    let [RulingKind::Stuck(Stuck::PushRefused { why })] = kinds.as_slice() else {
        panic!("a refused push was due: {kinds:?}");
    };
    assert!(why.contains("edits by maintainers are off"), "{why}");
    assert_ne!(worktree_head(&rig), head, "the commits are kept");
}

#[test]
fn the_push_is_never_forced() {
    let (rig, runner, url, head) = noted();
    // A worker that rewrote the contributor's commit: its head lacks theirs.
    let worktree = rig.paths().worktree(ticket(5));
    git(&worktree, &["reset", "--quiet", "--hard", "HEAD~1"]);
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    step(&runner).unwrap();
    rig.verdict(&runner);

    assert_eq!(on_fork(&url), head, "the contributor's head stands");
    let kinds = raised(&runner);
    assert!(
        matches!(
            kinds.as_slice(),
            [RulingKind::Stuck(Stuck::PushRefused { .. })]
        ),
        "{kinds:?}"
    );
}

#[test]
fn a_yes_on_a_refused_push_pushes_the_kept_commits_again() {
    let (rig, runner, url, head) = noted();
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    crate::test::write_script(&hook, "#!/bin/sh\nexit 1\n");
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    rig.verdict(&runner);
    assert_eq!(on_fork(&url), head, "refused");
    std::fs::remove_file(&hook).unwrap();

    runner
        .lock()
        .unwrap()
        .rule(ticket(80), Answer::Yes)
        .unwrap();
    rig.verdict(&runner);

    assert_eq!(on_fork(&url), worktree_head(&rig));
    assert_ne!(on_fork(&url), head);
}

#[test]
fn a_fork_head_that_moved_under_a_worktree_with_no_new_commits_is_still_moved() {
    let (rig, runner, url, head) = noted();
    assert_eq!(
        worktree_head(&rig),
        head,
        "the worker has committed nothing"
    );
    let (_, theirs) = rig.push_to_fork(FORK_BRANCH, "theirs.txt");

    let sent = runner.lock().unwrap().push_to_fork();

    assert_eq!(sent, Sent::Moved);
    assert_eq!(on_fork(&url), theirs, "their push stands");
}

#[test]
fn a_yes_on_the_foreign_change_merges_their_push_into_the_waiting_commits_and_pushes_them() {
    let (rig, runner, url, _) = noted();
    let (_, theirs) = rig.push_to_fork(FORK_BRANCH, "theirs.txt");
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    rig.verdict(&runner);
    let waiting = worktree_head(&rig);
    assert_ne!(on_fork(&url), waiting, "the commits wait");

    let answered = rig.ask(&runner, "rule", Some("80 yes"));
    assert_eq!(answered.get("error"), None, "{answered}");

    let merged = worktree_head(&rig);
    let parents = git(
        &rig.paths().worktree(ticket(5)),
        &["rev-list", "--parents", "-n1", "HEAD"],
    );
    assert!(
        parents.contains(&waiting) && parents.contains(&theirs),
        "a merge of both: {parents}"
    );
    rig.forge.set_checks(&merged, Checks::Passed);
    rig.claude.script([Scripted::Text(CLEAN)]);
    for _ in 0..12 {
        if on_fork(&url) == merged {
            break;
        }
        rig.verdict(&runner);
    }
    assert_eq!(on_fork(&url), merged, "kelpie pushed the merge");
}

#[test]
fn a_fix_the_fork_refuses_raises_push_refused_and_not_a_fix_not_pushed() {
    let (rig, runner) = contributed();
    let (url, head) = rig.push_to_fork(FORK_BRANCH, "fork.txt");
    rig.forge.set_fork(80, &url, true);
    release(&runner);
    assert_eq!(rig.ask(&runner, "adopt", Some("80")).get("error"), None);
    assert_eq!(step(&runner).unwrap(), Some(adopted()));
    // The first reviewer finds something, and the fork refuses what is pushed.
    rig.reviewer
        .script([ScriptedRound::Findings(vec![Finding::new(
            Severity::Medium,
            "fork.txt",
            1,
            "the flag is misnamed",
            "it reads as its opposite",
        )])]);
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    crate::test::write_script(&hook, "#!/bin/sh\necho 'edits are off' >&2\nexit 1\n");
    rig.forge.set_checks(&head, Checks::Passed);
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    let refused = (0..12).any(|_| {
        rig.verdict(&runner);
        raised(&runner)
            .iter()
            .any(|kind| matches!(kind, RulingKind::Stuck(Stuck::PushRefused { .. })))
    });

    assert!(refused, "the refusal was ruled on: {:?}", raised(&runner));
    assert!(
        !raised(&runner)
            .iter()
            .any(|kind| matches!(kind, RulingKind::Stuck(Stuck::FixNotPushed { .. }))),
        "no fix-not-pushed: {:?}",
        raised(&runner)
    );
    assert_eq!(on_fork(&url), head, "nothing was pushed");
}

#[test]
fn a_fix_the_fork_took_on_the_second_try_is_a_fix_pushed_and_not_a_fix_not_pushed() {
    let (rig, runner) = contributed();
    let (url, head) = rig.push_to_fork(FORK_BRANCH, "fork.txt");
    rig.forge.set_fork(80, &url, true);
    release(&runner);
    assert_eq!(rig.ask(&runner, "adopt", Some("80")).get("error"), None);
    assert_eq!(step(&runner).unwrap(), Some(adopted()));
    rig.reviewer
        .script([ScriptedRound::Findings(vec![Finding::new(
            Severity::Medium,
            "fork.txt",
            1,
            "the flag is misnamed",
            "it reads as its opposite",
        )])]);
    // The fork refuses the push at the end of the worker's turn, and takes the next.
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let once = Path::new(&url).join("refused-once");
    let body = format!(
        "#!/bin/sh\nif [ ! -e '{once}' ]; then touch '{once}'; echo 'try again' >&2; exit 1; fi\n",
        once = once.display()
    );
    crate::test::write_script(&hook, &body);
    rig.forge.set_checks(&head, Checks::Passed);
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    let pushed = (0..12).any(|_| {
        rig.verdict(&runner);
        on_fork(&url) != head
    });

    assert!(
        pushed,
        "the second try reached the fork: {:?}",
        raised(&runner)
    );
    assert_eq!(on_fork(&url), worktree_head(&rig));
    assert_eq!(raised(&runner), [], "no ruling over a fix that was pushed");
}

// The head a step fetched off the lock before the push is the fork's old one, which a step made
// again after the push would otherwise take as the fork's head
#[test]
fn a_push_to_the_fork_forgets_the_head_read_before_it() {
    let (_rig, runner, _, _) = noted();
    let mut held = runner.lock().unwrap();
    held.focus = Some(ticket(5));
    held.reads.git.keep(Found::Head(Ok("before".to_owned())));

    held.sent(80, Sent::Pushed("after".to_owned())).unwrap();

    assert!(held.reads.git.is_empty());
}

#[test]
fn a_pull_request_closed_by_hand_is_pushed_nothing_and_raises_closed_not_push_refused() {
    let (rig, runner, url, head) = noted();
    // The fork would refuse a push, as it does once its branch is gone.
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    crate::test::write_script(&hook, "#!/bin/sh\necho 'gone' >&2\nexit 1\n");
    // The worker commits, and the maintainer closes the pull request, while
    // it is at work.
    let hold = Hold::default();
    rig.claude.script([Scripted::Hold(hold.clone())]);
    std::thread::scope(|s| {
        let turn = s.spawn(|| step(&runner).unwrap());
        assert!(
            hold.entered(Duration::from_secs(10)),
            "the turn never began"
        );
        let worktree = rig.paths().worktree(ticket(5));
        std::fs::write(worktree.join("fix.txt"), "fixed\n").unwrap();
        git(&worktree, &["add", "fix.txt"]);
        let who = [
            "-c",
            "user.name=worker",
            "-c",
            "user.email=worker@example.com",
        ];
        git(
            &worktree,
            &[who.as_slice(), &["commit", "-m", "fix"]].concat(),
        );
        rig.forge.set_state(80, PullRequestState::Closed);
        hold.release();
        turn.join().unwrap();
    });
    rig.verdict(&runner);

    let kinds = raised(&runner);
    assert!(
        matches!(kinds.as_slice(), [RulingKind::Stuck(Stuck::Closed)]),
        "{kinds:?}"
    );
    assert_eq!(on_fork(&url), head, "nothing was pushed");
}

#[test]
fn a_commit_the_fork_refused_at_first_is_still_a_fix_sent_unread_on_purpose() {
    let (rig, runner, url, _) = noted();
    let hook = Path::new(&url).join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    crate::test::write_script(&hook, "#!/bin/sh\necho 'edits are off' >&2\nexit 1\n");
    rig.claude.script([Scripted::Commit("fix.txt", "fixed\n")]);

    step(&runner).unwrap();

    let committed = worktree_head(&rig);
    let runner = runner.lock().unwrap();
    let unread = &runner.current().unwrap().sent_unread;
    assert!(unread.contains(&committed), "{unread:?}");
}
