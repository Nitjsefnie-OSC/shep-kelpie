//! What a file is on this machine: whether it runs, and a link to it

use std::fs;
use std::io;
use std::path::Path;

/// Whether `meta`, a file's, says it can be run
///
/// On Unix that is an execute bit. Windows keeps none, so a regular file is
/// taken to run, and `host::on_path` looks at its extension.
#[cfg(unix)]
pub(in crate::adapters) fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    meta.permissions().mode() & 0o111 != 0
}

/// Whether `meta`, a file's, says it can be run
///
/// On Unix that is an execute bit. Windows keeps none, so a regular file is
/// taken to run, and `host::on_path` looks at its extension.
#[cfg(windows)]
pub(in crate::adapters) fn is_executable(meta: &fs::Metadata) -> bool {
    meta.is_file()
}

/// Makes `link` a symbolic link to `target`
///
/// On Windows that takes Developer Mode (#1271), and the kind of link follows
/// what `target` is.
#[cfg(unix)]
pub(in crate::adapters) fn link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Makes `link` a symbolic link to `target`
///
/// On Windows that takes Developer Mode (#1271), and the kind of link follows
/// what `target` is.
#[cfg(windows)]
pub(in crate::adapters) fn link(target: &Path, link: &Path) -> io::Result<()> {
    if fs::metadata(target)?.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

/// Removes the link `link` makes, whatever it points at
///
/// On Windows a link to a folder goes by `remove_dir`, since `remove_file`
/// refuses it.
#[cfg(unix)]
pub(in crate::adapters) fn unlink(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

/// Removes the link `link` makes, whatever it points at
///
/// On Windows a link to a folder goes by `remove_dir`, since `remove_file`
/// refuses it.
#[cfg(windows)]
pub(in crate::adapters) fn unlink(link: &Path) -> io::Result<()> {
    use std::os::windows::fs::FileTypeExt;

    if fs::symlink_metadata(link)?.file_type().is_symlink_dir() {
        fs::remove_dir(link)
    } else {
        fs::remove_file(link)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_file_runs_when_any_execute_bit_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tool");
        fs::write(&file, "").unwrap();
        for (mode, runs) in [(0o644, false), (0o744, true), (0o655, true)] {
            fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                is_executable(&fs::metadata(&file).unwrap()),
                runs,
                "{mode:o}"
            );
        }
    }

    #[test]
    fn a_link_to_a_file_or_a_folder_reads_through() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        let folder = dir.path().join("folder");
        fs::write(&file, "kept").unwrap();
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("inner"), "inside").unwrap();

        link(&file, &dir.path().join("to-file")).unwrap();
        link(&folder, &dir.path().join("to-folder")).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("to-file")).unwrap(),
            "kept"
        );
        let inner = dir.path().join("to-folder/inner");
        assert_eq!(fs::read_to_string(inner).unwrap(), "inside");
        let again = link(&file, &dir.path().join("to-file")).unwrap_err();
        assert_eq!(again.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn a_removed_link_leaves_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        let folder = dir.path().join("folder");
        fs::write(&file, "kept").unwrap();
        fs::create_dir(&folder).unwrap();
        for (target, name) in [(&file, "to-file"), (&folder, "to-folder")] {
            let made = dir.path().join(name);
            link(target, &made).unwrap();
            unlink(&made).unwrap();
            assert!(!made.is_symlink(), "{name}");
        }
        assert!(file.is_file() && folder.is_dir());
    }
}
