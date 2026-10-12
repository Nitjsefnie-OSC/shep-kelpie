//! Git's own housekeeping, which a worker's git skips
//!
//! The fence denies `packed-refs`, since every worktree shares one `.git`. Git
//! still tries to pack refs: `gc --auto` and `maintenance --auto` after enough
//! loose objects, and a fetch that prunes a ref it had packed. The lock it
//! takes beside the file fails with `packed-refs.lock: Operation not
//! permitted`. The commit or fetch itself went through, but the message reads
//! as a failure and workers spent turns on `git log` to be sure. So the
//! worker's git is told not to try. A commit locks the file on its own, which
//! no setting stops, so the fence lets git make `packed-refs.lock` and keeps
//! the file denied. A commit never writes the file, and git removes the lock.
//! A lock a killed git leaves in the shared `.git` is removed by
//! `worktree::prepare` once it is a minute old.

/// What `GIT_CONFIG_PARAMETERS` carries for every worker, whether or not
/// kelpie's App covers its repo
///
/// Git reads that variable after every config file, so these win over the
/// maintainer's and the repo's own.
///
/// The sandbox runtime sets the same variable to `http.proxyAuthMethod=basic`,
/// and this one replaces it, so it carries that setting too. Without it git
/// connects to the sandbox's proxy without its login, the proxy answers 407 and
/// closes the connection, and git's retry on that connection fails as "Proxy
/// CONNECT aborted": no covered worker could push.
pub(super) const PARAMETERS: &str =
    "'http.proxyAuthMethod'='basic' 'gc.auto'='0' 'maintenance.auto'='false' 'fetch.prune'='false'";

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    use super::PARAMETERS;

    const SETTINGS: [(&str, &str); 1] = [("GIT_CONFIG_PARAMETERS", PARAMETERS)];

    fn git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
        Command::new("git")
            .current_dir(dir)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", dir.parent().unwrap())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "w")
            .env("GIT_AUTHOR_EMAIL", "w@example.com")
            .env("GIT_COMMITTER_NAME", "w")
            .env("GIT_COMMITTER_EMAIL", "w@example.com")
            .envs(env.iter().copied())
            .output()
            .unwrap()
    }

    fn ok(dir: &Path, args: &[&str]) {
        let out = git(dir, args, &[]);
        assert!(out.status.success(), "{args:?}: {out:?}");
    }

    // A clone of a repo with a worktree, as a worker has, and the common git dir
    // shut to new files, so git cannot make `packed-refs.lock` beside the file
    // the fence denies. The clone asks for what a repo with many loose objects
    // and a pruning habit does.
    struct Fenced {
        root: tempfile::TempDir,
        origin: PathBuf,
        worktree: PathBuf,
    }

    impl Fenced {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let (origin, clone) = (root.path().join("origin"), root.path().join("clone"));
            fs::create_dir(&origin).unwrap();
            ok(&origin, &["init", "-q", "-b", "main"]);
            ok(&origin, &["commit", "-q", "--allow-empty", "-m", "init"]);
            ok(&origin, &["branch", "topic"]);
            ok(root.path(), &["clone", "-q", "origin", "clone"]);
            ok(&clone, &["config", "fetch.prune", "true"]);
            ok(&clone, &["worktree", "add", "-q", "../wt", "-b", "work"]);
            let worktree = root.path().join("wt");
            for n in 0..600 {
                fs::write(worktree.join(format!("f{n}")), n.to_string()).unwrap();
            }
            ok(&worktree, &["add", "."]);
            ok(&worktree, &["commit", "-q", "-m", "many files"]);
            // Loose objects enough for `gc --auto` to find work at a limit of 1,
            // set after the commit so that did not run it.
            ok(&clone, &["config", "gc.auto", "1"]);
            ok(&clone, &["config", "gc.autoDetach", "false"]);
            let shut = fs::Permissions::from_mode(0o555);
            fs::set_permissions(clone.join(".git"), shut).unwrap();
            Self {
                root,
                origin,
                worktree,
            }
        }

        fn commit(&self, env: &[(&str, &str)]) -> Output {
            fs::write(self.worktree.join("more"), "x").unwrap();
            git(&self.worktree, &["add", "."], env);
            git(&self.worktree, &["commit", "-q", "-m", "more"], env)
        }

        // The git dir takes new files again and `packed-refs` does not take a
        // write, as with the fence's lock allowed and the file denied.
        fn allow_the_lock_only(&self) {
            let clone = self.root.path().join("clone/.git");
            fs::set_permissions(&clone, fs::Permissions::from_mode(0o755)).unwrap();
            let packed = clone.join("packed-refs");
            fs::set_permissions(packed, fs::Permissions::from_mode(0o444)).unwrap();
        }

        fn gc_auto(&self, env: &[(&str, &str)]) -> String {
            stderr_of(&git(&self.worktree, &["gc", "--auto"], env))
        }

        // The branch `topic` is gone from the origin, and the fetch is told
        // nothing about pruning: the clone's config does.
        fn fetch_after_a_branch_is_deleted(&self, env: &[(&str, &str)]) -> String {
            ok(&self.origin, &["branch", "-D", "topic"]);
            stderr_of(&git(&self.worktree, &["fetch"], env))
        }
    }

    impl Drop for Fenced {
        fn drop(&mut self) {
            let open = fs::Permissions::from_mode(0o755);
            fs::set_permissions(self.root.path().join("clone/.git"), open).unwrap();
        }
    }

    fn stderr_of(out: &Output) -> String {
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    // Auto gc takes its own lock first, `gc.pid.lock`, then packs refs.
    #[test]
    fn reproduction_auto_gc_in_a_fenced_git_dir_prints_a_lock_message() {
        let stderr = Fenced::new().gc_auto(&[]);

        assert!(stderr.contains(".lock"), "{stderr}");
    }

    #[test]
    fn with_the_settings_auto_gc_prints_nothing() {
        let stderr = Fenced::new().gc_auto(&SETTINGS);

        assert_eq!(stderr, "");
    }

    // Measured on git 2.50.1: a commit deletes `CHERRY_PICK_HEAD` and
    // `REVERT_HEAD` when it is done, and deleting a ref locks `packed-refs`
    // first, though neither is there. No git setting stops it.
    #[test]
    fn reproduction_a_commit_where_git_cannot_make_the_lock_prints_the_lock_message() {
        let fenced = Fenced::new();

        let out = fenced.commit(&SETTINGS);

        assert!(out.status.success());
        assert!(stderr_of(&out).contains("packed-refs.lock"), "{out:?}");
    }

    // What the fence allows: the lock, and not the file, which a commit never
    // writes.
    #[test]
    fn a_commit_where_git_can_make_the_lock_prints_nothing_and_leaves_packed_refs_alone() {
        let fenced = Fenced::new();
        let packed = fenced.root.path().join("clone/.git/packed-refs");
        let before = fs::read_to_string(&packed).unwrap();
        fenced.allow_the_lock_only();

        let out = fenced.commit(&SETTINGS);

        assert!(out.status.success());
        assert_eq!(stderr_of(&out), "");
        assert_eq!(fs::read_to_string(&packed).unwrap(), before);
        assert!(!packed.with_extension("lock").exists());
    }

    #[test]
    fn reproduction_a_pruning_fetch_with_packed_refs_denied_prints_the_lock_message() {
        let stderr = Fenced::new().fetch_after_a_branch_is_deleted(&[]);

        assert!(stderr.contains("packed-refs.lock"), "{stderr}");
    }

    #[test]
    fn with_the_settings_a_fetch_prints_no_lock_message() {
        let stderr = Fenced::new().fetch_after_a_branch_is_deleted(&SETTINGS);

        assert!(!stderr.contains("packed-refs.lock"), "{stderr}");
    }
}
