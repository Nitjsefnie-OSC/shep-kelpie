//! The remote a work item's branch lives on
//!
//! A branch kelpie opened lives on `origin`. A contributor's lives on their
//! fork, which is not a remote of the project's repo: it is fetched from and
//! pushed to by URL. What is fetched from a fork lands under
//! `refs/remotes/fork-<hash of the URL>/`, so it cannot overwrite a ref of
//! `origin`'s, `origin/main` included.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{WorktreeError, git};

/// Where a work item's branch is pushed, and fetched from
// wire format: a work item saves it, so changing this is a breaking change to
// the state file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Remote {
    /// The project's `origin`, under the work item's own branch name
    #[default]
    Origin,
    /// A contributor's fork, whose branch has a name of its own
    Fork {
        /// Where the fork is fetched from and pushed to
        url: String,
        /// The branch on the fork
        branch: String,
    },
}

impl Remote {
    /// Whether this is the project's `origin`
    pub fn is_origin(&self) -> bool {
        *self == Self::Origin
    }

    /// Where git is told to fetch from and push to
    pub(super) fn target(&self) -> &str {
        match self {
            Self::Origin => "origin",
            Self::Fork { url, .. } => url,
        }
    }

    /// The branch's full name on the remote, `branch` being the work item's
    pub(super) fn branch_ref(&self, branch: &str) -> String {
        match self {
            Self::Origin => format!("refs/heads/{branch}"),
            Self::Fork { branch: theirs, .. } => format!("refs/heads/{theirs}"),
        }
    }

    /// The ref the remote's branch is fetched to
    pub(super) fn tracking(&self, branch: &str) -> String {
        match self {
            Self::Origin => format!("refs/remotes/origin/{branch}"),
            Self::Fork {
                url,
                branch: theirs,
            } => {
                format!("refs/remotes/{}/{theirs}", fork_name(url))
            }
        }
    }

    /// Fetches the branch, and returns the ref it was fetched to
    pub(super) fn fetch(&self, repo: &Path, branch: &str) -> Result<String, WorktreeError> {
        let tracking = self.tracking(branch);
        match self {
            Self::Origin => git(repo, ["fetch", "--quiet", "--", "origin", branch])?,
            Self::Fork { url, .. } => {
                // The leading `+` takes a fork's force push.
                let spec = format!("+{}:{tracking}", self.branch_ref(branch));
                git(repo, ["fetch", "--quiet", "--", url, &spec])?
            }
        };
        Ok(tracking)
    }

    /// Fetches `origin/main` and the branch together, and returns the branch's ref
    pub(super) fn fetch_with_main(
        &self,
        repo: &Path,
        branch: &str,
        base: &str,
    ) -> Result<String, WorktreeError> {
        if *self == Self::Origin {
            git(repo, ["fetch", "--quiet", "--", "origin", base, branch])?;
            return Ok(self.tracking(branch));
        }
        git(repo, ["fetch", "--quiet", "--", "origin", base])?;
        self.fetch(repo, branch)
    }
}

// A name for a fork's tracking refs that is the same for the same URL, and a
// single short folder whatever the URL holds.
fn fork_name(url: &str) -> String {
    let hash = url.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("fork-{hash:016x}")
}

#[cfg(test)]
mod tests;
