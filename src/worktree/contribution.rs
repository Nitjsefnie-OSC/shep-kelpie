//! A contribution's head, fetched and diffed without checking anything out
//!
//! Another person's pull request is code kelpie has not read. Fetching its
//! head moves objects into the project's checkout and nothing else: no
//! branch, no worktree, no file on disk. The diff is read from those objects
//! with every program a config or an attribute could name switched off.

use std::path::Path;
use std::process::Stdio;

use super::{BASE, WorktreeError, git, in_repo};

/// What a model cannot read in a change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opaque {
    /// A file git calls binary
    Binary,
    /// A symbolic link, which points at a path rather than holding content
    Symlink,
    /// A submodule, which pins code the diff does not hold
    Submodule,
}

impl Opaque {
    /// What it is, in words
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Binary => "a binary change",
            Self::Symlink => "a symbolic link",
            Self::Submodule => "a submodule change",
        }
    }
}

/// One path in a change whose content a model cannot read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    /// The path
    pub path: String,
    /// Why a model cannot read it
    pub kind: Opaque,
}

/// A contribution's change, as far as one head is read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The text of the diff
    pub diff: String,
    /// The paths in it a model cannot read
    pub opaque: Vec<Touched>,
}

/// The change contribution `pull_request`'s `head` makes
///
/// It is the change since `passed`, the last head that passed the security
/// review, when `passed` is an ancestor of `head`. With none, or when it is
/// not an ancestor (a force push, or a commit git no longer holds), it is the
/// whole of `head`'s change from `origin/main`. Fetches `refs/pull/<n>/head`
/// and `main` first, and checks nothing out.
///
/// # Errors
///
/// [`WorktreeError`] naming the git command that failed, or the commit the
/// fetch did not bring.
pub fn change(
    repo: &Path,
    pull_request: u64,
    head: &str,
    passed: Option<&str>,
) -> Result<Change, WorktreeError> {
    fetch(repo, pull_request, head)?;
    let since = match passed {
        Some(passed) if descends(repo, passed, head)? => format!("{passed}..{head}"),
        _ => format!("origin/{BASE}...{head}"),
    };
    let modes = diff_of(repo, &["--raw", "-z"], &since)?;
    let counts = diff_of(repo, &["--numstat", "-z"], &since)?;
    let mut opaque = by_mode(&modes);
    for touched in binary(&counts) {
        if !opaque.iter().any(|o| o.path == touched.path) {
            opaque.push(touched);
        }
    }
    Ok(Change {
        diff: diff_of(repo, &[], &since)?,
        opaque,
    })
}

// Brings `pull_request`'s head and `main` into `repo`'s objects, and checks
// that `head` is among them. `FETCH_HEAD` is all it writes besides `main`'s
// remote-tracking ref.
fn fetch(repo: &Path, pull_request: u64, head: &str) -> Result<(), WorktreeError> {
    let hash = |h: &str| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit());
    if !hash(head) {
        return Err(WorktreeError::Git {
            args: "rev-parse".into(),
            stderr: format!("{head:?} is not a commit hash"),
        });
    }
    let pull = format!("refs/pull/{pull_request}/head");
    git(
        repo,
        ["fetch", "--quiet", "--no-tags", "origin", BASE, &pull],
    )?;
    let commit = format!("{head}^{{commit}}");
    git(repo, ["rev-parse", "--verify", "--quiet", &commit]).map(drop)
}

// Whether `passed` is an ancestor of `head`. A commit git does not hold is not
// one, so the whole change is read.
fn descends(repo: &Path, passed: &str, head: &str) -> Result<bool, WorktreeError> {
    let hash = !passed.is_empty() && passed.bytes().all(|b| b.is_ascii_hexdigit());
    if !hash {
        return Ok(false);
    }
    let output = in_repo(repo)
        .args(["merge-base", "--is-ancestor", passed, head])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| WorktreeError::Spawn(e.to_string()))?;
    Ok(output.status.code() == Some(0))
}

// `git diff` of `since`, shaped by `shape`, whole: `git` trims what it
// returns. Neither a program a config names nor one an attribute picks turns
// the diff into something else, and a rename is a delete and an add, so each
// file's content is in the diff.
fn diff_of(repo: &Path, shape: &[&str], since: &str) -> Result<String, WorktreeError> {
    let flags = [
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--no-color",
    ];
    let args = ["diff"].into_iter().chain(shape.iter().copied());
    let args: Vec<&str> = args.chain(flags).chain([since]).collect();
    let output = in_repo(repo)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| WorktreeError::Spawn(e.to_string()))?;
    if !output.status.success() {
        return Err(WorktreeError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// The symlinks and submodules in `git diff --raw -z`, whose entries are
// `:<old mode> <new mode> <old hash> <new hash> <status>` and then the path.
fn by_mode(raw: &str) -> Vec<Touched> {
    let mut found = Vec::new();
    let mut fields = raw.split('\0');
    while let Some(entry) = fields.next() {
        let Some(entry) = entry.strip_prefix(':') else {
            continue;
        };
        let Some(path) = fields.next() else { break };
        let mut parts = entry.split(' ');
        let modes = [parts.next(), parts.next()];
        let kind = match modes {
            m if m.contains(&Some("120000")) => Opaque::Symlink,
            m if m.contains(&Some("160000")) => Opaque::Submodule,
            _ => continue,
        };
        found.push(Touched {
            path: path.to_owned(),
            kind,
        });
    }
    found
}

// The binary files in `git diff --numstat -z`, whose entries are
// `<added>\t<deleted>\t<path>`, both counts `-` for a binary file.
fn binary(numstat: &str) -> Vec<Touched> {
    numstat
        .split('\0')
        .filter_map(|entry| entry.strip_prefix("-\t-\t"))
        .map(|path| Touched {
            path: path.to_owned(),
            kind: Opaque::Binary,
        })
        .collect()
}
