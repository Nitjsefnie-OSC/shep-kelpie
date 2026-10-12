//! Putting the results the host sent into the worktree, past no link
//!
//! rsync writes the results into a folder of the dog's own, and this copies
//! them from there. The worktree is the worker's to change while the copy
//! goes, so a path checked and then used could be a link by the time it is
//! used. Each folder is instead opened from the one before it with
//! `O_NOFOLLOW`, and each file is created through the folder it is in, so a
//! link anywhere on the way is refused wherever it appears, and nothing is
//! written through it.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, FileType, Mode, OFlags, mkdirat, openat, statat};
use rustix::io::Errno;

/// Copies what is in `staging` into the `results` folder of `worktree`,
/// making the folder if it is not there, leaving what else is in it, and
/// skipping any link in `staging`
///
/// # Errors
///
/// A message when `results` or a folder above it in the worktree is a link,
/// or when a folder or file cannot be made or written.
pub(super) fn deliver(staging: &Path, worktree: &Path, results: &Path) -> Result<(), String> {
    let root = rustix::fs::open(
        worktree,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| format!("cannot open {}: {e}", worktree.display()))?;
    let mut into = root;
    let mut shown = Path::new("").to_owned();
    for part in results {
        shown.push(part);
        into = make_and_open(&into, part, &shown)?;
    }
    copy_tree(staging, &into, &shown)
}

// Opens folder `name` of `parent`, making it first if it is not there, and
// refuses it when it is a link.
fn make_and_open(parent: &OwnedFd, name: &OsStr, shown: &Path) -> Result<OwnedFd, String> {
    match mkdirat(parent, name, FOLDER) {
        Ok(()) | Err(Errno::EXIST) => {}
        Err(e) => return Err(format!("cannot make {}: {e}", shown.display())),
    }
    openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| {
        let found = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW);
        if found.is_ok_and(|s| FileType::from_raw_mode(s.st_mode) == FileType::Symlink) {
            format!(
                "{} in the worktree is a link, and results are not copied through one",
                shown.display()
            )
        } else {
            format!("cannot open {} in the worktree: {e}", shown.display())
        }
    })
}

const FOLDER: Mode = Mode::RWXU
    .union(Mode::RGRP)
    .union(Mode::XGRP)
    .union(Mode::ROTH)
    .union(Mode::XOTH);

const FILE: Mode = Mode::RUSR
    .union(Mode::WUSR)
    .union(Mode::RGRP)
    .union(Mode::ROTH);

fn copy_tree(from: &Path, into: &OwnedFd, shown: &Path) -> Result<(), String> {
    let read = |e: io::Error| format!("cannot read {}: {e}", from.display());
    for entry in std::fs::read_dir(from).map_err(read)? {
        let entry = entry.map_err(read)?;
        let name = entry.file_name();
        let shown = shown.join(&name);
        let kind = entry.file_type().map_err(read)?;
        if kind.is_dir() {
            let inner = make_and_open(into, &name, &shown)?;
            copy_tree(&entry.path(), &inner, &shown)?;
        } else if kind.is_file() {
            copy_file(&entry.path(), into, &name, &shown)?;
        }
    }
    Ok(())
}

fn copy_file(from: &Path, into: &OwnedFd, name: &OsStr, shown: &Path) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("cannot write {}: {e}", shown.display());
    let mut source = File::open(from).map_err(|e| failed(&e))?;
    let fd = openat(
        into,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        FILE,
    )
    .map_err(|e| failed(&e))?;
    let mut dest = File::from(fd);
    io::copy(&mut source, &mut dest).map_err(|e| failed(&e))?;
    // rsync kept the host's times; so does this.
    if let Ok(modified) = source.metadata().and_then(|m| m.modified()) {
        let _ = dest.set_modified(modified);
    }
    Ok(())
}
