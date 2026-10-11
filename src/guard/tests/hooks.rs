//! The project's own hooks, run by `kelpie guard` on a worker's commit, push
//! and `gh` text: the same hooks for every harness, as the guard is theirs

use super::*;
use crate::settings::{HookPoint, NonBlank, ProjectHook};

const WORD: &str = "Acme";

// Refuses a payload naming the word, with a reason that does not repeat it.
const REFUSE_WORD: &str =
    "if grep -q Acme; then echo 'it names a word the project keeps private' >&2; exit 2; fi";

fn hooks(list: &[(HookPoint, &str)]) -> Hooks {
    let list: Vec<ProjectHook> = list
        .iter()
        .map(|&(point, command)| ProjectHook {
            point,
            command: NonBlank::try_from(command.to_owned()).unwrap(),
        })
        .collect();
    Hooks::new("koji", "acme/koji", Path::new("/"), &list)
}

impl WorkerTree {
    fn bash_hooked(&self, hooks: &Hooks, command: &str) -> Verdict {
        let call = json!({
            "tool_name": "Bash",
            "cwd": self.path(),
            "tool_input": { "command": command },
        });
        let common = self.repo().join(".git");
        let worktree = self.path();
        let checkout = Checkout {
            git_common_dir: &common,
            worktree: &worktree,
        };
        judge_with(
            call.to_string().as_bytes(),
            Some(Path::new(HOME)),
            local(Some(Path::new(HOME))),
            hooks,
            checkout,
        )
    }

    // Commits a file, which is what a push then sends.
    fn commit_file(&self, text: &str, message: &str) {
        self.write("notes.md", text);
        self.git(&["add", "notes.md"]);
        self.git(&["commit", "--quiet", "-m", message]);
    }
}

#[test]
fn a_commit_adding_the_word_is_refused_by_the_hook_with_its_reason() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Commit, REFUSE_WORD)]);
    tree.write("notes.md", "built for Acme\n");
    tree.git(&["add", "notes.md"]);

    let why = refusal(tree.bash_hooked(&hooks, "git commit -m 'docs: notes'"));

    assert!(
        why.contains(
            "this commit: the `commit` hook `if` refused: it names a word the project keeps \
             private"
        ),
        "{why}"
    );
    assert!(!why.contains(WORD), "{why}");
}

#[test]
fn a_commit_message_naming_the_word_is_refused_by_the_hook() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Commit, REFUSE_WORD)]);
    tree.write("notes.md", "fine\n");
    tree.git(&["add", "notes.md"]);

    let why = refusal(tree.bash_hooked(&hooks, "git commit -m 'docs: for Acme'"));

    assert!(why.contains("refused"), "{why}");
    assert_eq!(
        tree.bash_hooked(&hooks, "git commit -m 'docs: for a customer'"),
        Verdict::Allow
    );
}

#[test]
fn a_commit_the_hook_passes_goes_through_and_other_points_do_not_judge_it() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Push, "exit 2")]);
    tree.write("notes.md", "built for Acme\n");
    tree.git(&["add", "notes.md"]);

    assert_eq!(
        tree.bash_hooked(&hooks, "git commit -m 'docs: notes'"),
        Verdict::Allow
    );
}

#[test]
fn a_hook_reads_the_commit_s_message_and_staged_diff() {
    let tree = WorkerTree::new();
    let seen = tree.0.path().join("seen.json");
    let record = format!("cat > {}", seen.display());
    let hooks = hooks(&[(HookPoint::Commit, &record)]);
    tree.write("notes.md", "a line\n");
    tree.git(&["add", "notes.md"]);

    assert_eq!(
        tree.bash_hooked(&hooks, "git commit -m 'docs: notes'"),
        Verdict::Allow
    );

    let payload: serde_json::Value = serde_json::from_slice(&fs::read(seen).unwrap()).unwrap();
    assert_eq!(payload["point"], "commit");
    assert_eq!(payload["project"], "koji");
    assert_eq!(payload["role"], "worker");
    assert_eq!(payload["repo"], "acme/koji");
    assert_eq!(payload["message"], "docs: notes");
    let diff = payload["diff"].as_str().unwrap();
    assert!(diff.contains("+++ b/notes.md"), "{diff}");
    assert!(diff.contains("+a line"), "{diff}");
}

#[test]
fn a_push_sending_the_word_is_refused_by_the_hook() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Push, REFUSE_WORD)]);
    tree.commit_file("built for Acme\n", "docs: notes");

    let why = refusal(tree.bash_hooked(&hooks, "git push origin HEAD"));

    assert!(
        why.contains("this push: the `push` hook `if` refused: it names a word"),
        "{why}"
    );
}

#[test]
fn a_hook_reads_the_commits_a_push_sends() {
    let tree = WorkerTree::new();
    let seen = tree.0.path().join("seen.json");
    let record = format!("cat > {}", seen.display());
    let hooks = hooks(&[(HookPoint::Push, &record)]);
    tree.commit_file("a line\n", "docs: notes");

    assert_eq!(
        tree.bash_hooked(&hooks, "git push origin HEAD"),
        Verdict::Allow
    );

    let payload: serde_json::Value = serde_json::from_slice(&fs::read(seen).unwrap()).unwrap();
    assert_eq!(payload["point"], "push");
    let [commit] = payload["commits"].as_array().unwrap().as_slice() else {
        panic!("one commit is sent: {payload}");
    };
    assert_eq!(commit["message"], "docs: notes");
    let (base, head) = (payload["base"].as_str(), payload["head"].as_str());
    assert_eq!(commit["sha"].as_str(), head);
    assert_eq!(
        payload["range"].as_str().map(str::to_owned),
        Some(format!("{}..{}", base.unwrap(), head.unwrap()))
    );
    assert!(payload["diff"].as_str().unwrap().contains("+a line"));
}

#[test]
fn a_push_of_nothing_new_runs_no_hook() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Push, "exit 2")]);

    assert_eq!(
        tree.bash_hooked(&hooks, "git push origin HEAD"),
        Verdict::Allow
    );
}

#[test]
fn text_a_worker_posts_with_gh_is_judged_by_the_publish_hook() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Publish, REFUSE_WORD)]);

    for command in [
        "gh pr create --title 'feat: x' --body 'built for Acme'",
        "gh pr create --title 'feat: x' --body-file - <<'EOF'\nbuilt for Acme\nEOF",
        "gh pr edit 3 --title 'feat: for Acme'",
    ] {
        let why = refusal(tree.bash_hooked(&hooks, command));
        assert!(
            why.contains("the `publish` hook `if` refused: it names a word"),
            "{command}: {why}"
        );
        assert!(!why.contains(WORD), "{command}: {why}");
    }
    assert_eq!(
        tree.bash_hooked(
            &hooks,
            "gh pr create --title 'feat: x' --body 'built for a customer'"
        ),
        Verdict::Allow
    );
}

#[test]
fn a_hook_that_fails_refuses_the_call_and_says_it_failed() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Commit, "echo oops >&2; exit 1")]);
    tree.write("notes.md", "fine\n");
    tree.git(&["add", "notes.md"]);

    let why = refusal(tree.bash_hooked(&hooks, "git commit -m 'docs: notes'"));

    assert!(
        why.contains("the `commit` hook `echo` failed, and a check that cannot run refuses"),
        "{why}"
    );
    assert!(why.contains("it exited with 1: oops"), "{why}");
}

#[test]
fn a_hook_missing_from_the_machine_refuses_the_call() {
    let tree = WorkerTree::new();
    let hooks = hooks(&[(HookPoint::Commit, "kelpie-no-such-hook")]);
    tree.write("notes.md", "fine\n");
    tree.git(&["add", "notes.md"]);

    let why = refusal(tree.bash_hooked(&hooks, "git commit -m 'docs: notes'"));

    assert!(why.contains("failed"), "{why}");
}

#[test]
fn the_guard_reads_its_hooks_from_its_command_line() {
    let list = hooks(&[(HookPoint::Commit, REFUSE_WORD)]);
    let flag = list.flag().unwrap();

    let (back, rest) = Hooks::take(&[format!("{FOLDER_FLAG}/x"), flag]).unwrap();

    assert_eq!(rest, [format!("{FOLDER_FLAG}/x")]);
    // The hooks a command line brings run like the ones it was made from.
    let refused = back.check(
        HookPoint::Commit,
        crate::hooks::Role::Worker,
        None,
        json!({ "diff": "+Acme" }),
    );
    assert!(refused.is_err());
    let passed = back.check(
        HookPoint::Commit,
        crate::hooks::Role::Worker,
        None,
        json!({ "diff": "+fine" }),
    );
    assert_eq!(passed, Ok(()));
}
