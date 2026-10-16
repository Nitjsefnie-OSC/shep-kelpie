//! A contributor's fork for the rig's pull requests

use super::*;

impl Rig {
    /// Pushes a commit of `file` to `branch` on a fork of the project, from
    /// a clone of origin, and returns the fork's URL and the commit
    ///
    /// The fork is a bare repository beside origin, made on the first call. A
    /// second call to the same branch adds to it.
    pub(crate) fn push_to_fork(&self, branch: &str, file: &str) -> (String, String) {
        let fork = self.home.path().join("fork.git");
        if !fork.exists() {
            git(
                self.home.path(),
                &["init", "--quiet", "--bare", "-b", "main", path(&fork)],
            );
        }
        let clone = self.home.path().join("forker");
        if !clone.exists() {
            git(
                self.home.path(),
                &["clone", "--quiet", path(&self.origin()), path(&clone)],
            );
        }
        let known = git(&clone, &["ls-remote", path(&fork), branch]);
        git(&clone, &["fetch", "--quiet", "origin"]);
        let start = match known.is_empty() {
            true => "origin/main".to_owned(),
            false => {
                git(&clone, &["fetch", "--quiet", path(&fork), branch]);
                "FETCH_HEAD".to_owned()
            }
        };
        git(&clone, &["checkout", "--quiet", "-B", branch, &start]);
        std::fs::write(clone.join(file), "pushed to the fork\n").unwrap();
        git(&clone, &["add", file]);
        git(&clone, &["commit", "--quiet", "-m", file]);
        git(&clone, &["push", "--quiet", path(&fork), branch]);
        (path(&fork).to_owned(), git(&clone, &["rev-parse", "HEAD"]))
    }
}
