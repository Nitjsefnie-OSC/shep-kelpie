//! The copy on Unix, through `openat` and `unlinkat`

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, FileType, Mode, OFlags, mkdirat, openat, statat, unlinkat};
use rustix::io::Errno;

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
    // What is there is unlinked, not opened: opening a hard link would write
    // through it to a file outside the worktree, and opening a FIFO would
    // block this thread for good. A new file is made with O_EXCL, so one put
    // back in between is refused too.
    match unlinkat(into, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => {}
        Err(e) => return Err(failed(&e)),
    }
    let fd = openat(
        into,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
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
