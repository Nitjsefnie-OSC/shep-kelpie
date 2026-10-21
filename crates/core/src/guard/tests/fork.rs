//! A work item on a contributor's fork, whose commits kelpie pushes itself

use super::*;

// The tree's call, judged for a worker whose push kelpie makes for it
fn committing(tree: &WorkerTree, command: &str) -> Verdict {
    let common = tree.repo().join(".git");
    let worktree = tree.path();
    let checkout = Checkout {
        git_common_dir: &common,
        worktree: &worktree,
        base: "main",
        kelpie_pushes: true,
    };
    call(&worktree, command, checkout)
}

#[test]
fn a_fork_workers_push_is_refused_by_the_guard_and_it_is_told_to_commit_only() {
    let tree = WorkerTree::new();
    for command in [
        "git push origin HEAD",
        "git push",
        "git push origin kelpie/7",
        "git push --force-with-lease origin HEAD",
        "git -C . push origin HEAD",
        "cd . && git push origin HEAD",
    ] {
        let why = refusal(committing(&tree, command));
        assert!(
            why.contains("commit") && why.contains("Kelpie pushes"),
            "{command}: {why}"
        );
    }
}

#[test]
fn a_fork_worker_still_commits_and_reads_its_git() {
    let tree = WorkerTree::new();
    tree.write("work.txt", "work\n");
    for command in [
        "git add work.txt && git commit -m 'fix(parser): keep the last line'",
        "git status",
        "git log --oneline",
        "git merge origin/main",
    ] {
        assert_eq!(committing(&tree, command), Verdict::Allow, "{command}");
    }
}

#[test]
fn a_repos_own_branch_still_pushes_as_before() {
    let tree = WorkerTree::new();
    assert_eq!(tree.bash("git push origin HEAD"), Verdict::Allow);
}

#[test]
fn the_flag_rides_the_guards_command_line_and_is_taken_off_it() {
    let words = [
        "--folder=/k".to_owned(),
        KELPIE_PUSHES_FLAG.to_owned(),
        "--pin-folder".to_owned(),
    ];
    let (pushes, rest) = kelpie_pushes_flag(&words);
    assert!(pushes);
    assert_eq!(rest, ["--folder=/k", "--pin-folder"]);
    assert!(!kelpie_pushes_flag(&rest).0);
}
