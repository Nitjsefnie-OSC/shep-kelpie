//! Folders and files only their owner can use, and the check that one is
//!
//! On Unix that is mode 0700 and 0600, and a check of the owner's uid and
//! the mode. On Windows it is a protected DACL holding one ACE, for the
//! current user, and a check of the owner SID and every ACE.

use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;
#[cfg(not(any(unix, windows)))]
compile_error!("kelpie keeps a file to its owner on Unix and Windows only");

/// Makes `folder` and any folder above it, the new ones usable by their
/// owner alone
///
/// A folder that is already there is left as it is.
///
/// # Errors
///
/// The OS's error when a folder cannot be made or narrowed.
pub fn private_dir(folder: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut at = Some(folder);
    while let Some(path) = at.filter(|path| !path.as_os_str().is_empty()) {
        if path.is_dir() {
            break;
        }
        missing.push(path);
        at = path.parent();
    }
    for path in missing.into_iter().rev() {
        match imp::create_dir(path) {
            Ok(()) => {}
            // Another process made it first: its access is its own.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Makes the file `path`, which must not exist, writable and readable by
/// its owner alone
///
/// # Errors
///
/// The OS's error when the file cannot be made, which is
/// [`io::ErrorKind::AlreadyExists`] when there is one.
pub fn create_private(path: &Path) -> io::Result<fs::File> {
    imp::create_file(path)
}

/// Narrows the folder `path`, and what it will hold, to its owner alone
///
/// # Errors
///
/// The OS's error when the folder cannot be narrowed, which is not the
/// user's to narrow when someone else owns it.
pub fn restrict_dir(path: &Path) -> io::Result<()> {
    imp::restrict_dir(path)
}

/// Whether the current user owns `path`
///
/// # Errors
///
/// The OS's error when `path` cannot be read.
pub fn is_owner(path: &Path) -> io::Result<bool> {
    imp::is_owner(path)
}

/// Whether the current user owns `path` and no one else can use it
///
/// On Unix that is no mode bit for group or other. On Windows it is a DACL
/// that grants no one but the current user, and none at all is everyone.
///
/// # Errors
///
/// The OS's error when `path` cannot be read.
pub fn is_owner_only(path: &Path) -> io::Result<bool> {
    imp::is_owner_only(path)
}

#[cfg(test)]
mod tests;
