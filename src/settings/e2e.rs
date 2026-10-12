//! The host that runs a project's end-to-end tests, `[app.dogs.kelpie.e2e]`

use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a project's end-to-end tests run, which a worker's sandbox cannot do
///
/// The host must be disposable. The project's `command` runs there as the
/// ssh alias's account, and a worker edits that script, so whatever the
/// account can reach, a worker can reach through it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "E2E")]
pub struct E2e {
    /// The host, as an alias from your ssh config: letters, digits, `.`, `_`
    /// and `-`, not starting with `-`. It must be disposable: the project's
    /// script runs there as the alias's account, and a worker edits that
    /// script.
    pub host: SshAlias,
    /// The script that runs the tests, a file in the repo: relative, with no
    /// `..`. `scripts/e2e-remote.sh` when absent.
    #[serde(default = "default_command")]
    pub command: RepoPath,
    /// The folder in the repo the run leaves its results in, which are
    /// copied back into the worker's worktree: relative, with no `..`, and
    /// not `.git`. `test-results` when absent.
    #[serde(default = "default_results")]
    pub results: ResultsDir,
}

fn default_command() -> RepoPath {
    RepoPath(PathBuf::from("scripts/e2e-remote.sh"))
}

fn default_results() -> ResultsDir {
    ResultsDir(RepoPath(PathBuf::from("test-results")))
}

/// An ssh alias: letters, digits, `.`, `_` and `-`, not starting with `-`
///
/// It is passed to `ssh` as an argument, so a leading `-` would read as an option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the shape goes here.
#[schemars(extend("pattern" = "^[A-Za-z0-9._][A-Za-z0-9._-]*$"))]
pub struct SshAlias(String);

impl SshAlias {
    /// The alias as written
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SshAlias {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
        if value.is_empty() || value.starts_with('-') || !value.chars().all(ok) {
            return Err(
                "must be an ssh alias: letters, digits, `.`, `_` and `-`, not starting with `-`",
            );
        }
        Ok(Self(value))
    }
}

/// A path inside the repo: relative, with no `..`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
pub struct RepoPath(PathBuf);

impl RepoPath {
    /// The path, relative to the repo
    #[inline]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl TryFrom<String> for RepoPath {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // A `.` component names the folder it is in, so it is dropped.
        let mut path = PathBuf::new();
        for part in Path::new(&value).components() {
            match part {
                Component::Normal(name) => path.push(name),
                Component::CurDir => {}
                _ => return Err("must be a relative path inside the repo, with no `..`"),
            }
        }
        if path.as_os_str().is_empty() {
            return Err("must be a relative path inside the repo, with no `..`");
        }
        Ok(Self(path))
    }
}

/// A folder inside the repo that results are copied into: a [`RepoPath`] that
/// is not `.git` or inside it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
pub struct ResultsDir(RepoPath);

impl ResultsDir {
    /// The folder, relative to the repo
    #[inline]
    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }
}

impl TryFrom<String> for ResultsDir {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let path = RepoPath::try_from(value)?;
        if path.as_path().starts_with(".git") {
            return Err("must not be `.git` or inside it");
        }
        Ok(Self(path))
    }
}
