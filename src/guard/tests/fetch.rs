//! A worker's `git fetch` in the shared clone, answered before it runs
//!
//! Every work item's worktree shares one `.git`, and a worker may write only
//! its own branch's refs. A bare `git fetch` updates every
//! `refs/remotes/origin/*`, so each other branch's lock fails and git exits 1,
//! which a worker spent turns reading as a broken sandbox. Kelpie fetches
//! `origin/main` itself, so the guard answers the attempt instead.

use super::*;

const FETCHED: &str = "kelpie fetches `origin/main`";
const MERGE: &str = "`git merge origin/main`";

fn refused_with(verdict: Verdict, part: &str, command: &str) {
    let why = refusal(verdict);
    assert!(why.contains(part), "{command}: {why}");
}

#[test]
fn a_fetch_in_the_work_item_s_repo_is_answered_with_what_kelpie_does() {
    let tree = WorkerTree::new();
    tree.write("src/main.rs", "fn main() {}\n");
    for command in [
        "git fetch",
        "git fetch origin",
        "git fetch --quiet origin main",
        "git fetch --all",
        "git fetch --tags",
        "git pull",
        "git pull origin main",
        "git pull --ff-only",
        "git -C . fetch",
        "cd src && git fetch",
        "bash -c 'git fetch origin'",
        "env git fetch origin",
    ] {
        let why = refusal(tree.bash(command));
        assert!(why.contains(FETCHED), "{command}: {why}");
        assert!(why.contains(MERGE), "{command}: {why}");
    }
}

// A fetch that names only one ref still writes a ref of `origin`'s, which is
// kelpie's to move, and a tag of what it fetched.
#[test]
fn a_fetch_of_one_ref_of_origins_is_answered_the_same_way() {
    let tree = WorkerTree::new();
    for command in [
        "git fetch origin main",
        "git fetch origin kelpie/7",
        "git fetch origin refs/heads/main:refs/remotes/origin/main",
    ] {
        refused_with(tree.bash(command), FETCHED, command);
    }
}

// The worker's own branch is the one ref of `origin`'s it may write, and
// `git merge` of what kelpie fetched needs no fetch of its own.
#[test]
fn what_a_worker_does_with_the_refs_kelpie_fetched_goes_through() {
    let tree = WorkerTree::new();
    tree.git(&["push", "--quiet", "origin", "HEAD:refs/heads/kelpie/7"]);
    for command in [
        "git merge origin/main",
        "git merge --no-edit origin/main",
        "git log origin/main..HEAD",
        "git ls-remote origin",
        "git push origin HEAD",
    ] {
        assert_eq!(tree.bash(command), Verdict::Allow, "{command}");
    }
}

// A repo the worker made inside its worktree is its own: its refs are writable
// there, and a test fixture that fetches from a local origin is ordinary work.
#[test]
fn a_fetch_in_a_repo_the_worker_made_goes_through() {
    let tree = WorkerTree::new();
    tree.git(&["init", "--quiet", "--bare", "fixture/origin"]);
    tree.git(&["clone", "--quiet", "fixture/origin", "fixture/clone"]);
    tree.git(&[
        "-C",
        "fixture/clone",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "one",
    ]);
    tree.git(&[
        "-C",
        "fixture/clone",
        "push",
        "--quiet",
        "origin",
        "HEAD:refs/heads/main",
    ]);

    assert_eq!(tree.bash("cd fixture/clone && git fetch"), Verdict::Allow);
    assert_eq!(
        tree.bash("git -C fixture/clone fetch origin"),
        Verdict::Allow
    );
    assert_eq!(tree.bash("cd fixture/clone && git pull"), Verdict::Allow);
}

// A word in a program kelpie does not know may only name git.
#[test]
fn a_fetch_that_only_a_program_names_is_not_refused() {
    let tree = WorkerTree::new();
    assert_eq!(tree.bash("rg 'git fetch'"), Verdict::Allow);
}
