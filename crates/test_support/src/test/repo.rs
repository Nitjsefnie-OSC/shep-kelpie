// Folders and git repos a test builds, with the maintainer's own git config kept out

use std::path::{Path, PathBuf};
use std::process::Command;

/// Writes `text` to `file` under `folder`, making the folders between
///
/// # Panics
///
/// When a folder or the file cannot be made.
pub fn write_in(folder: &Path, file: &str, text: &str) {
    let path = folder.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn path(p: &Path) -> &str {
    p.to_str().expect("a UTF-8 temporary path")
}

/// Runs git in `cwd` and returns its trimmed stdout
///
/// The maintainer's own git config never reaches the rig: no signing, no hooks.
///
/// # Panics
///
/// When git cannot run, or exits with a failure.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=rig",
            "-c",
            "user.email=rig@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "git {args:?} in {}: {stderr}",
        cwd.display()
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repo at `home/repo` with one commit on `main`, and a worktree of it at
/// `home/wt` on its own branch, the way kelpie cuts one
///
/// # Panics
///
/// When a folder cannot be made or a git call fails.
pub fn linked_worktree(home: &Path) -> (PathBuf, PathBuf) {
    let repo = home.join("repo");
    let worktree = home.join("wt");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    git(&repo, &["commit", "--quiet", "--allow-empty", "-m", "init"]);
    git(
        &repo,
        &["worktree", "add", "--quiet", "-b", "wt", path(&worktree)],
    );
    (repo, worktree)
}
