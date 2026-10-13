//! A contribution's head, fetched, diffed, and checked out for a reader
//!
//! Another person's pull request is code kelpie has not read. Fetching its
//! head moves objects into the project's checkout and nothing else: no
//! branch, no worktree, no file on disk. The diff is read from those objects
//! with every program a config or an attribute could name switched off.
//! [`checkout`] is the one call that puts its files on disk, in a detached
//! worktree with hooks off, only for a head that has passed the security
//! review, and only for a reader that runs no code in it.

use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use super::{BASE, WorktreeError, git, in_repo, output, remove_view};

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
    /// The text of the diff, cut at the limit it was read with
    pub diff: String,
    /// Whether the diff was longer than that limit, so `diff` is cut short and
    /// `opaque` is not read
    pub cut: bool,
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
/// Reads at most `limit` bytes of any git output and stops git there, so a
/// huge change from outside costs no more than that. A diff longer than
/// `limit` comes back cut, with `cut` set and no `opaque` paths.
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
    limit: usize,
) -> Result<Change, WorktreeError> {
    fetch(repo, pull_request, head)?;
    let since = match passed {
        Some(passed) if descends(repo, passed, head)? => format!("{passed}..{head}"),
        _ => format!("origin/{BASE}...{head}"),
    };
    let (diff, cut) = diff_of(repo, &[], &since, limit)?;
    if cut {
        // Flagged for its length whatever else is in it: nothing more to read.
        return Ok(Change {
            diff,
            cut,
            opaque: Vec::new(),
        });
    }
    let (modes, _) = diff_of(repo, &["--raw", "-z"], &since, limit)?;
    let (counts, _) = diff_of(repo, &["--numstat", "-z"], &since, limit)?;
    let mut opaque = by_mode(&modes);
    for touched in binary(&counts) {
        if !opaque.iter().any(|o| o.path == touched.path) {
            opaque.push(touched);
        }
    }
    Ok(Change { diff, cut, opaque })
}

/// Makes `path` a detached worktree of `repo` at contribution
/// `pull_request`'s `head`, fetched from `refs/pull/<n>/head`, for a reader
/// that opens its files and changes nothing
///
/// A worktree already at `path` is replaced. The checkout runs none of the
/// repo's hooks, and `main` is fetched too, so the change is
/// `origin/main...HEAD` there.
///
/// # Errors
///
/// [`WorktreeError`] naming the git command or folder that failed, or the
/// commit the fetch did not bring.
pub fn checkout(
    repo: &Path,
    pull_request: u64,
    head: &str,
    path: &Path,
) -> Result<(), WorktreeError> {
    fetch(repo, pull_request, head)?;
    remove_view(repo, path)?;
    if let Some(parent) = path.parent() {
        super::create(parent)?;
    }
    let args = ["-c", "core.hooksPath=/dev/null"].map(OsStr::new);
    let add = ["worktree", "add", "--quiet", "--detach"].map(OsStr::new);
    output(
        in_repo(repo),
        args.into_iter()
            .chain(add)
            .chain([path.as_os_str(), OsStr::new(head)]),
    )
    .map(drop)
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

// `git diff` of `since`, shaped by `shape`, whole up to `limit` bytes, and
// whether it was longer. Neither a program a config names nor one an
// attribute picks turns the diff into something else, and a rename is a
// delete and an add, so each file's content is in the diff.
fn diff_of(
    repo: &Path,
    shape: &[&str],
    since: &str,
    limit: usize,
) -> Result<(String, bool), WorktreeError> {
    let flags = [
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--no-color",
    ];
    let args = ["diff"].into_iter().chain(shape.iter().copied());
    let args: Vec<&str> = args.chain(flags).chain([since]).collect();
    let mut git = in_repo(repo);
    git.args(&args);
    let (out, cut) = capped(git, limit)?;
    match out {
        Ok(stdout) => Ok((String::from_utf8_lossy(&stdout).into_owned(), cut)),
        Err(stderr) => Err(WorktreeError::Git {
            args: args.join(" "),
            stderr,
        }),
    }
}

// Runs `git`, keeping at most `limit` bytes of its stdout. Past that it kills
// it and says the output was cut. The inner result is the output, or stderr
// when git failed on its own.
fn capped(
    mut git: Command,
    limit: usize,
) -> Result<(Result<Vec<u8>, String>, bool), WorktreeError> {
    let spawn = |e: std::io::Error| WorktreeError::Spawn(e.to_string());
    let mut child = (git.stdin(Stdio::null()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(spawn)?;
    let stderr = child.stderr.take();
    let errors = thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut text);
        }
        text
    });
    let mut out = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let ask = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
        stdout.take(ask).read_to_end(&mut out).map_err(spawn)?;
    }
    let cut = out.len() > limit;
    if cut {
        out.truncate(limit);
        let _ = child.kill();
    }
    let status = child.wait().map_err(spawn)?;
    let stderr = errors.join().unwrap_or_default();
    if cut || status.success() {
        return Ok((Ok(out), cut));
    }
    Ok((Err(String::from_utf8_lossy(&stderr).into_owned()), false))
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
