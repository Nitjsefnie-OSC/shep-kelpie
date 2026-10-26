//! What a commit holds of the harnesses' files, and where a branch left
//! `main`, each read once per commit
//!
//! A commit's tree and ancestors never change, so a result is kept by the
//! commit's id and never by a ref, which moves.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use super::{folds_to, root_names};
use crate::worktree::{self, WorktreeError, git};

/// A fenced file's mode and blob, by its path
pub(super) type Files = BTreeMap<String, (String, String)>;

/// Results kept by `K`, each read at most once
struct Kept<K, V>(Mutex<BTreeMap<K, V>>);

impl<K: Ord, V: Clone> Kept<K, V> {
    const fn new() -> Self {
        Self(Mutex::new(BTreeMap::new()))
    }

    fn get_or_read(
        &self,
        key: K,
        read: impl FnOnce() -> Result<V, WorktreeError>,
    ) -> Result<V, WorktreeError> {
        if let Some(kept) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(kept.clone());
        }
        let value = read()?;
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(kept.entry(key).or_insert(value).clone())
    }
}

static FILES: Kept<(PathBuf, String), Arc<Files>> = Kept::new();
static CUT_AT: Kept<(PathBuf, String, String), String> = Kept::new();

// Only a full object id names one commit for good: a shorter one may
// come to match another.
fn is_full_id(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The commit `origin/<base>` is at now
pub(super) fn main_of(repo: &Path, base: &str) -> Result<String, WorktreeError> {
    git(repo, ["rev-parse", &main_ref(base)])
}

/// The commits `worktree` is at and `origin/<base>` is at now, from one git call
pub(super) fn head_and_main(
    repo: &Path,
    worktree: &Path,
    base: &str,
) -> Result<(String, String), WorktreeError> {
    let args = ["rev-parse", "HEAD", &main_ref(base)];
    let ids = worktree::trusted(repo, worktree)?(&args)?;
    match ids.split_once('\n') {
        Some((head, main)) => Ok((head.to_owned(), main.to_owned())),
        None => Err(WorktreeError::Git {
            args: args.join(" "),
            stderr: format!("printed {ids:?}, not two commits"),
        }),
    }
}

fn main_ref(base: &str) -> String {
    format!("origin/{base}^{{commit}}")
}

/// The commit `head` left `main` at, where `main` is a commit id
pub(super) fn cut_at(repo: &Path, head: &str, main: &str) -> Result<String, WorktreeError> {
    let key = (repo.to_owned(), head.to_owned(), main.to_owned());
    let read = || git(repo, ["merge-base", head, main]);
    if is_full_id(head) && is_full_id(main) {
        CUT_AT.get_or_read(key, read)
    } else {
        read()
    }
}

/// Each fenced file in `commit`, by path with its root folder named as
/// `on_disk` names it, with its mode and blob
pub(super) fn files_in(repo: &Path, commit: &str) -> Result<Arc<Files>, WorktreeError> {
    let read = || in_commit(repo, commit).map(Arc::new);
    if is_full_id(commit) {
        FILES.get_or_read((repo.to_owned(), commit.to_owned()), read)
    } else {
        read()
    }
}

// Git matches a pathspec by exact case, so the root is listed and matched
// here. Two spellings of one path never match anything.
fn in_commit(repo: &Path, commit: &str) -> Result<Files, WorktreeError> {
    let entries = |listing: String| -> Vec<(String, String, String)> {
        listing
            .split('\0')
            .filter_map(|line| {
                let (meta, path) = line.split_once('\t')?;
                let mut meta = meta.split(' ');
                let (mode, _, blob) = (meta.next()?, meta.next()?, meta.next()?);
                Some((path.to_owned(), mode.to_owned(), blob.to_owned()))
            })
            .collect()
    };
    let mut found = BTreeMap::new();
    for (root, ..) in entries(git(repo, ["ls-tree", "-z", commit])?) {
        let Some(name) = root_names().find(|name| folds_to(OsStr::new(&root), name)) else {
            continue;
        };
        let args = [
            "--literal-pathspecs",
            "ls-tree",
            "-r",
            "-z",
            commit,
            "--",
            &root,
        ];
        for (path, mode, blob) in entries(git(repo, args)?) {
            let key = format!("{name}{}", &path[root.len()..]);
            let entry = (mode, blob);
            let conflict = || ("conflict".to_owned(), String::new());
            found
                .entry(key)
                .and_modify(|seen| *seen = conflict())
                .or_insert(entry);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn a_result_is_read_once_per_key_and_a_failed_read_is_read_again() {
        let kept: Kept<&str, u8> = Kept::new();
        let reads = Cell::new(0);
        let read = |value| {
            reads.set(reads.get() + 1);
            Ok(value)
        };

        assert_eq!(kept.get_or_read("a", || read(1)), Ok(1));
        assert_eq!(kept.get_or_read("a", || read(9)), Ok(1));
        assert_eq!(kept.get_or_read("b", || read(2)), Ok(2));
        assert_eq!(reads.get(), 2);

        let spawn = || Err(WorktreeError::Spawn("no git".into()));
        assert!(kept.get_or_read("c", spawn).is_err());
        assert_eq!(kept.get_or_read("c", || read(3)), Ok(3));
    }

    #[test]
    fn only_a_full_object_id_names_a_commit_for_good() {
        assert!(is_full_id(ID));
        assert!(is_full_id(&ID.repeat(2)[..64]));
        assert!(!is_full_id("origin/main"));
        assert!(!is_full_id(&ID[..7]));
        assert!(!is_full_id(&ID.replace('0', "g")));
    }
}
