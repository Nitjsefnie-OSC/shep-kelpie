//! A project whose `git.base` names a branch other than `main`: its work
//! items are cut from that branch, and their pull requests open against it and
//! merge into it

use super::*;
use crate::ports::Checks;
use crate::test::Scripted;

// Issue 7's worker pushes, leaving the file kelpie opens its pull request from.
fn opens_a_pull_request(rig: &Rig, runner: &Mutex<Runner>) -> u64 {
    fs::create_dir_all(rig.build_7()).unwrap();
    fs::write(
        rig.build_7().join("pull-request.md"),
        "feat: x\n\nWhy.\n\nResolves #7\n",
    )
    .unwrap();
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(runner).unwrap();
    rig.ask(runner, "status", None)["work_item"]["pull_request"]
        .as_u64()
        .expect("a pull request was opened")
}

#[test]
fn a_worker_branch_is_cut_from_the_base_and_not_from_main() {
    let rig = Rig::new("shep");
    rig.base_is("next");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);

    step(&runner).unwrap();

    let worktree = rig.home.path().join("shep/kelpie/shep/worktrees/7");
    assert_eq!(git(&worktree, &["branch", "--show-current"]), "kelpie/7");
    let next = git(&rig.repo(), &["rev-parse", "origin/next"]);
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), next);
    assert_ne!(next, git(&rig.repo(), &["rev-parse", "origin/main"]));
    assert!(
        worktree.join("NEXT.md").is_file(),
        "the base's file is there"
    );
}

#[test]
fn a_pull_request_opens_against_the_base_and_merges_there_with_main_left_alone() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.base_is("next");
    rig.merge_auto();
    let main = git(&rig.repo(), &["rev-parse", "origin/main"]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));

    let number = opens_a_pull_request(&rig, &runner);

    assert_eq!(rig.forge.base_of(number), "next");
    rig.claude.script([
        Scripted::Text(crate::ports::CLEAN),
        Scripted::Text(crate::ports::CLEAN),
    ]);
    for _ in 0..40 {
        if let Some(head) = rig.forge.head_of("kelpie/7") {
            rig.forge.set_checks(&head, Checks::Passed);
        }
        rig.verdict(&runner);
        if !rig.forge.merges().is_empty() {
            break;
        }
    }
    let [(merged, _)] = rig.forge.merges().try_into().expect("one merge");
    assert_eq!(merged, number);
    assert_eq!(rig.forge.base_of(merged), "next");
    assert_eq!(rig.forge.head_of("main").as_deref(), Some(main.as_str()));
}

#[test]
fn a_project_with_no_base_cuts_and_opens_against_main() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));

    let number = opens_a_pull_request(&rig, &runner);

    assert_eq!(rig.forge.base_of(number), "main");
    let worktree = rig.home.path().join("shep/kelpie/shep/worktrees/7");
    let log = git(&worktree, &["log", "--format=%H", "origin/main..HEAD"]);
    assert_eq!(log.lines().count(), 1, "one commit past origin/main: {log}");
}

#[test]
fn a_worktree_cut_from_the_old_base_still_loads_its_own_claude_files_once_the_base_moves() {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n\n"));
    rig.land(".claude/skills/house/SKILL.md", "Rules.\n");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    // `dev` carries a `.claude` file the item's worktree, cut from `main`, does not.
    rig.base_is("dev");
    let repo = rig.repo();
    git(&repo, &["checkout", "--quiet", "dev"]);
    let skill = repo.join(".claude/skills/house/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(skill, "Other rules.\n").unwrap();
    git(&repo, &["add", ".claude"]);
    git(&repo, &["commit", "--quiet", "-m", "change the skill"]);
    git(&repo, &["push", "--quiet", "origin", "dev"]);
    git(&repo, &["checkout", "--quiet", "main"]);
    runner
        .lock()
        .unwrap()
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);

    step(&runner).unwrap();

    let own = rig.paths().skills.join("repo/kelpie-repo");
    let calls = rig.claude.calls();
    let [call] = calls.as_slice() else {
        panic!("one worker call: {calls:?}")
    };
    assert!(
        !call.plugin_dirs.contains(&own),
        "the worktree's own files load once: {:?}",
        call.plugin_dirs
    );
}
