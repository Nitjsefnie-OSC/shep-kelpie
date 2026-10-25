//! The copy on Windows, through folders held open on the way down
//!
//! Windows has no `openat`, so each folder is opened by its path, from the
//! worktree down, and every handle is held until the copy ends. A handle that
//! shares no delete access keeps its folder from being renamed or replaced, so
//! the folder under it is the one that was checked. A folder that is a reparse
//! point (a symlink or a junction) is refused, and a file is made with
//! `create_new` after the old one is removed, so a link put in its place
//! fails the write.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

// The Win32 header values, kept here so the dog takes no crate for five numbers.
// `FILE_FLAG_*` are in winbase.h, `FILE_SHARE_*` and `FILE_ATTRIBUTE_*` in winnt.h.
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;

/// Copies what is in `staging` into the `results` folder of `worktree`,
/// making the folder if it is not there, leaving what else is in it, and
/// skipping any link in `staging`
///
/// # Errors
///
/// A message when `results` or a folder above it in the worktree is a link,
/// or when a folder or file cannot be made or written.
pub(in crate::dog::e2e::run) fn deliver(
    staging: &Path,
    worktree: &Path,
    results: &Path,
) -> Result<(), String> {
    let root = options()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(worktree)
        .map_err(|e| format!("cannot open {}: {e}", worktree.display()))?;
    // Held, not read: every folder on the way stays open until the copy ends.
    let mut held = vec![root];
    let mut at = worktree.to_owned();
    let mut shown = PathBuf::new();
    for part in results {
        at.push(part);
        shown.push(part);
        held.push(make_and_open(&at, &shown)?);
    }
    copy_tree(staging, &at, &shown)
}

// No delete access is shared: that is what keeps an open folder in its place.
fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    options
}

// Opens the folder at `path`, making it first if it is not there, and refuses
// it when it is a link. The check is on the handle that was opened, which is
// the link itself and not what it points to.
fn make_and_open(path: &Path, shown: &Path) -> Result<File, String> {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("cannot make {}: {e}", shown.display())),
    }
    let failed = |e: io::Error| format!("cannot open {} in the worktree: {e}", shown.display());
    let folder = options()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(failed)?;
    let found = folder.metadata().map_err(failed)?;
    if found.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "{} in the worktree is a link, and results are not copied through one",
            shown.display()
        ));
    }
    if !found.is_dir() {
        return Err(failed(io::Error::from(io::ErrorKind::NotADirectory)));
    }
    Ok(folder)
}

fn copy_tree(from: &Path, into: &Path, shown: &Path) -> Result<(), String> {
    let read = |e: io::Error| format!("cannot read {}: {e}", from.display());
    for entry in std::fs::read_dir(from).map_err(read)? {
        let entry = entry.map_err(read)?;
        let name = entry.file_name();
        let shown = shown.join(&name);
        let kind = entry.file_type().map_err(read)?;
        if kind.is_dir() {
            let at = into.join(&name);
            let _held = make_and_open(&at, &shown)?;
            copy_tree(&entry.path(), &at, &shown)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), &into.join(&name), &shown)?;
        }
    }
    Ok(())
}

fn copy_file(from: &Path, to: &Path, shown: &Path) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("cannot write {}: {e}", shown.display());
    let mut source = File::open(from).map_err(|e| failed(&e))?;
    // What is there is removed, not opened: opening a hard link would write
    // through it to a file outside the worktree. A new file is made with
    // `create_new`, so a link put back in between is refused too.
    match std::fs::remove_file(to) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(failed(&e)),
    }
    let mut dest = options()
        .write(true)
        .create_new(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(to)
        .map_err(|e| failed(&e))?;
    io::copy(&mut source, &mut dest).map_err(|e| failed(&e))?;
    // rsync kept the host's times; so does this.
    if let Ok(modified) = source.metadata().and_then(|m| m.modified()) {
        let _ = dest.set_modified(modified);
    }
    Ok(())
}
