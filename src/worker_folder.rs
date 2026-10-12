//! Reading and writing files in a folder a worker can write
//!
//! Kelpie runs outside the sandbox, as the maintainer, and a worker can
//! plant a link anywhere in its build folder or worktree. A path opened the
//! ordinary way follows it to any file the maintainer can read or write: a
//! `pull-request.md` that links the GitHub App's key would be posted as a pull
//! request's body, and a `pull-request.applied.md` that links a shell's rc
//! file would be overwritten with the worker's text. So every file kelpie
//! reads or writes in such a folder goes through here, and nothing here
//! follows a link.
//!
//! - A read opens the file with `O_NOFOLLOW`, then asks the open file what it
//!   is, and takes only a regular file the user owns, of a size [`MAX`] or
//!   less. It opens without blocking, so a FIFO is refused, not waited on.
//! - A write puts the text in a new file made with `O_EXCL` and `O_NOFOLLOW`
//!   beside the file, then renames it over the file, which replaces a link
//!   there and leaves what it pointed to alone.
//! - A path with folders in it is walked one folder at a time from the root,
//!   each opened with `O_NOFOLLOW` and `O_DIRECTORY`, and the file is opened
//!   and renamed relative to the last one's descriptor. A folder swapped for a
//!   link while kelpie works is then never followed either.
//!
//! The root is the one folder opened by its path as given: it is kelpie's own,
//! and the worker cannot replace it.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, mkdirat, openat, renameat, unlinkat};
use rustix::io::Errno;

/// The most a file read from a worker's folder may hold, in bytes
pub(crate) const MAX: u64 = 1 << 20;

/// Why a file in a worker's folder was not read or written
///
/// None of its text holds a file's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    /// It is a link or hard link, a FIFO, a device, a folder, or owned by someone else
    NotRegular,
    /// A folder on the way to it is a link or not a folder
    NotFolder,
    /// It holds more than [`MAX`] bytes
    TooBig,
    /// The system refused for this reason
    Io(io::ErrorKind),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegular => f.write_str("it is not a regular file, so kelpie leaves it"),
            Self::NotFolder => f.write_str("a folder on its path is not a plain folder"),
            Self::TooBig => write!(f, "it is over {MAX} bytes"),
            Self::Io(kind) => write!(f, "{kind}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(kind) => kind.into(),
            e => Self::other(e.to_string()),
        }
    }
}

impl From<Errno> for Error {
    fn from(e: Errno) -> Self {
        Self::Io(io::Error::from(e).kind())
    }
}

/// The text of `rel` below `root`, or none when it, or a folder on the way to
/// it, is not there
///
/// # Errors
///
/// [`Error::NotRegular`] or [`Error::NotFolder`] for a link or anything else
/// that is not a plain file in plain folders, [`Error::TooBig`] for a file over
/// [`MAX`], and [`Error::Io`] for the rest, which includes text that is not
/// UTF-8.
pub(crate) fn read(root: &Path, rel: &Path) -> Result<Option<String>, Error> {
    let Some((dir, name)) = walk(root, rel, false)? else {
        return Ok(None);
    };
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::NOCTTY;
    let fd = match openat(&dir, &name, flags | OFlags::CLOEXEC, Mode::empty()) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(Errno::LOOP | Errno::NOTDIR) => return Err(Error::NotRegular),
        Err(e) => return Err(e.into()),
    };
    let stat = fstat(&fd)?;
    let regular = FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile;
    // A file with another name too is a hard link, which O_NOFOLLOW cannot see: the
    // worker could link a file of the maintainer's into its folder.
    let ours = stat.st_uid == rustix::process::geteuid().as_raw() && stat.st_nlink == 1;
    if !regular || !ours {
        return Err(Error::NotRegular);
    }
    if u64::try_from(stat.st_size).map_or(true, |size| size > MAX) {
        return Err(Error::TooBig);
    }
    let mut text = String::new();
    // One byte past the cap, so a file that grew since `fstat` is caught.
    File::from(fd)
        .take(MAX + 1)
        .read_to_string(&mut text)
        .map_err(|e| Error::Io(e.kind()))?;
    if text.len() as u64 > MAX {
        return Err(Error::TooBig);
    }
    Ok(Some(text))
}

/// Writes `text` to `rel` below `root`, making the root and the folders on the
/// way, and replacing a file or link there
///
/// # Errors
///
/// [`Error::NotFolder`] for a link or a file where a folder should be, and
/// [`Error::Io`] for the rest.
pub(crate) fn write(root: &Path, rel: &Path, text: &str) -> Result<(), Error> {
    put(root, rel, text, Mode::from_bits_truncate(0o666))
}

/// As [`write`], for the user alone, whatever mode a file there had
///
/// # Errors
///
/// As [`write`].
pub(crate) fn write_private(root: &Path, rel: &Path, text: &str) -> Result<(), Error> {
    put(root, rel, text, Mode::RUSR | Mode::WUSR)
}

fn put(root: &Path, rel: &Path, text: &str, mode: Mode) -> Result<(), Error> {
    std::fs::create_dir_all(root).map_err(|e| Error::Io(e.kind()))?;
    let (dir, name) = walk(root, rel, true)?.ok_or(Error::Io(io::ErrorKind::NotFound))?;
    let mut beside = name.clone();
    beside.push(".tmp");
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(&dir, &beside, flags, mode) {
        Ok(fd) => fd,
        // A file or link left there, by a crash or the worker. Removing it
        // removes a link itself, never what it names.
        Err(Errno::EXIST) => {
            unlinkat(&dir, &beside, AtFlags::empty())?;
            openat(&dir, &beside, flags, mode)?
        }
        Err(e) => return Err(e.into()),
    };
    let mut file = File::from(fd);
    let written = file.write_all(text.as_bytes());
    drop(file);
    written
        .map_err(|e| Error::Io(e.kind()))
        .and_then(|()| Ok(renameat(&dir, &beside, &dir, &name)?))
        .inspect_err(|_| {
            let _ = unlinkat(&dir, &beside, AtFlags::empty());
        })
}

// The folder holding `rel` below `root`, opened by descriptor, and the file's
// name in it. None when a folder on the way is not there and `make` is false.
fn walk(root: &Path, rel: &Path, make: bool) -> Result<Option<(OwnedFd, OsString)>, Error> {
    let mut names = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(name) => names.push(name),
            _ => return Err(Error::Io(io::ErrorKind::InvalidInput)),
        }
    }
    let Some((file, folders)) = names.split_last() else {
        return Err(Error::Io(io::ErrorKind::InvalidInput));
    };
    let mut dir = match rustix::fs::open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(dir) => dir,
        Err(Errno::NOENT) if !make => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    for folder in folders {
        dir = match enter(&dir, folder, make)? {
            Some(next) => next,
            None => return Ok(None),
        };
    }
    Ok(Some((dir, (*file).to_owned())))
}

// `name` in `dir`, as a folder, made first when `make`; a link there is refused
fn enter(dir: &OwnedFd, name: &OsStr, make: bool) -> Result<Option<OwnedFd>, Error> {
    if make {
        match mkdirat(dir, name, Mode::from_bits_truncate(0o777)) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match openat(dir, name, flags, Mode::empty()) {
        Ok(next) => Ok(Some(next)),
        Err(Errno::NOENT) => Ok(None),
        Err(Errno::LOOP | Errno::NOTDIR) => Err(Error::NotFolder),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    // A build folder, and beside it a file that is none of the worker's
    fn folders() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (build, secret) = (dir.path().join("build"), dir.path().join("secret"));
        fs::create_dir(&build).unwrap();
        fs::write(&secret, "key\n").unwrap();
        (dir, build, secret)
    }

    // The call, on a thread, so a read that waits fails the test rather than hang it
    fn within_a_second<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || tx.send(call()));
        rx.recv_timeout(Duration::from_secs(1))
            .expect("the call hung")
    }

    #[test]
    fn a_plain_file_is_read_and_a_missing_one_is_none() {
        let (_dir, build, _) = folders();
        fs::write(build.join("a.md"), "text\n").unwrap();

        assert_eq!(read(&build, Path::new("a.md")), Ok(Some("text\n".into())));
        assert_eq!(read(&build, Path::new("b.md")), Ok(None));
        assert_eq!(read(&build, Path::new("gone/b.md")), Ok(None));
        assert_eq!(read(&build.join("gone"), Path::new("b.md")), Ok(None));
    }

    #[test]
    fn a_link_to_a_file_outside_the_folder_is_refused_without_its_content() {
        let (_dir, build, secret) = folders();
        symlink(&secret, build.join("a.md")).unwrap();

        let refused = read(&build, Path::new("a.md"));

        assert_eq!(refused, Err(Error::NotRegular));
        assert!(!refused.unwrap_err().to_string().contains("key"));
    }

    // A hard link is a regular file with the user's name on it, which O_NOFOLLOW
    // does not see.
    #[test]
    fn a_hard_link_to_a_file_outside_the_folder_is_refused_without_its_content() {
        let (_dir, build, secret) = folders();
        fs::hard_link(&secret, build.join("a.md")).unwrap();

        let refused = read(&build, Path::new("a.md"));

        assert_eq!(refused, Err(Error::NotRegular));
        assert!(!refused.unwrap_err().to_string().contains("key"));
    }

    #[test]
    fn a_written_file_has_the_mode_an_ordinary_write_gives_under_the_same_umask() {
        let (_dir, build, _) = folders();
        fs::write(build.join("ordinary.md"), "x").unwrap();

        write(&build, Path::new("a.md"), "x").unwrap();

        let mode = |name: &str| fs::metadata(build.join(name)).unwrap().permissions().mode();
        assert_eq!(mode("a.md"), mode("ordinary.md"));
    }

    #[test]
    fn a_link_to_a_missing_file_is_refused_too() {
        let (dir, build, _) = folders();
        symlink(dir.path().join("nothing"), build.join("a.md")).unwrap();

        assert_eq!(read(&build, Path::new("a.md")), Err(Error::NotRegular));
    }

    #[test]
    fn a_fifo_is_refused_and_not_waited_on() {
        let (_dir, build, _) = folders();
        let made = Command::new("mkfifo").arg(build.join("a.md")).status();
        assert!(made.unwrap().success());

        let refused = within_a_second(move || read(&build, Path::new("a.md")));

        assert_eq!(refused, Err(Error::NotRegular));
    }

    #[test]
    fn a_folder_is_refused_as_a_file() {
        let (_dir, build, _) = folders();
        fs::create_dir(build.join("a.md")).unwrap();

        assert_eq!(read(&build, Path::new("a.md")), Err(Error::NotRegular));
    }

    #[test]
    fn a_file_over_the_cap_is_refused_and_one_at_it_is_read() {
        let (_dir, build, _) = folders();
        let at = "x".repeat(usize::try_from(MAX).unwrap());
        fs::write(build.join("at.md"), &at).unwrap();
        fs::write(build.join("over.md"), format!("{at}x")).unwrap();

        assert_eq!(read(&build, Path::new("at.md")), Ok(Some(at)));
        assert_eq!(read(&build, Path::new("over.md")), Err(Error::TooBig));
    }

    #[test]
    fn a_folder_that_is_a_link_is_not_followed_to_read_or_write() {
        let (dir, build, _) = folders();
        let theirs = dir.path().join("theirs");
        fs::create_dir(&theirs).unwrap();
        fs::write(theirs.join("hosts.yml"), "theirs\n").unwrap();
        symlink(&theirs, build.join("gh")).unwrap();
        let file = Path::new("gh/hosts.yml");

        assert_eq!(read(&build, file), Err(Error::NotFolder));
        assert_eq!(write(&build, file, "mine\n"), Err(Error::NotFolder));
        assert_eq!(write_private(&build, file, "mine\n"), Err(Error::NotFolder));

        assert_eq!(
            fs::read_to_string(theirs.join("hosts.yml")).unwrap(),
            "theirs\n"
        );
        assert_eq!(fs::read_dir(&theirs).unwrap().count(), 1);
    }

    #[test]
    fn a_write_replaces_a_link_and_leaves_what_it_names_alone() {
        let (_dir, build, secret) = folders();
        symlink(&secret, build.join("a.md")).unwrap();

        write(&build, Path::new("a.md"), "mine\n").unwrap();

        assert_eq!(fs::read_to_string(&secret).unwrap(), "key\n");
        assert!(
            !fs::symlink_metadata(build.join("a.md"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(build.join("a.md")).unwrap(), "mine\n");
    }

    #[test]
    fn a_link_planted_where_the_write_stages_its_file_is_not_written_through() {
        let (_dir, build, secret) = folders();
        symlink(&secret, build.join("a.md.tmp")).unwrap();

        write(&build, Path::new("a.md"), "mine\n").unwrap();

        assert_eq!(fs::read_to_string(&secret).unwrap(), "key\n");
        assert_eq!(fs::read_to_string(build.join("a.md")).unwrap(), "mine\n");
        assert!(!build.join("a.md.tmp").exists());
    }

    #[test]
    fn a_staged_file_a_crash_left_does_not_stop_the_next_write() {
        let (_dir, build, _) = folders();
        fs::write(build.join("a.md.tmp"), "half").unwrap();

        write(&build, Path::new("a.md"), "whole\n").unwrap();

        assert_eq!(fs::read_to_string(build.join("a.md")).unwrap(), "whole\n");
    }

    #[test]
    fn a_write_makes_the_folders_on_its_way_and_the_private_one_is_for_the_user_alone() {
        let (_dir, build, _) = folders();
        let file = Path::new("gh/deep/hosts.yml");
        write_private(&build, file, "first\n").unwrap();
        fs::set_permissions(build.join(file), fs::Permissions::from_mode(0o644)).unwrap();

        write_private(&build, file, "second\n").unwrap();

        assert_eq!(fs::read_to_string(build.join(file)).unwrap(), "second\n");
        let mode = fs::metadata(build.join(file)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_path_that_climbs_out_of_the_folder_is_refused() {
        let (_dir, build, _) = folders();

        assert!(read(&build, Path::new("../secret")).is_err());
        assert!(write(&build, Path::new("/etc/x"), "").is_err());
        assert!(write(&build, Path::new(""), "").is_err());
    }
}
