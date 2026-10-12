// A temp folder short enough to hold a socket, under `TMPDIR` so a worker's
// sandbox, which writes nowhere else, can make it.

use std::path::Path;

/// The longest folder that still leaves room for a forwarder's socket: a
/// separator, a 32-character name and `.sock` (38 bytes) under the 103 bytes
/// macOS allows a socket path
const LONGEST_FOLDER: usize = 65;

/// A new folder under `TMPDIR`, named by four random characters
///
/// # Panics
///
/// When the folder cannot be made, or `TMPDIR` is so deep that a socket in
/// the folder would pass the platform's limit.
#[track_caller]
pub(crate) fn short_tempdir() -> tempfile::TempDir {
    short_tempdir_in(&std::env::temp_dir())
}

#[track_caller]
fn short_tempdir_in(parent: &Path) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("")
        .rand_bytes(4)
        .tempdir_in(parent)
        .unwrap();
    let length = dir.path().canonicalize().unwrap().as_os_str().len();
    assert!(
        length <= LONGEST_FOLDER,
        "{} is {length} bytes, over {LONGEST_FOLDER}: a socket in it would be too long a path; \
         set TMPDIR to a shorter folder",
        dir.path().display()
    );
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_made_under_tmpdir_and_removed_with_its_handle() {
        let dir = short_tempdir();
        let path = dir.path().to_owned();
        assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    #[should_panic(expected = "a socket in it would be too long a path")]
    fn a_parent_too_deep_for_a_socket_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let deep = parent.path().join("d".repeat(LONGEST_FOLDER));
        std::fs::create_dir(&deep).unwrap();
        let _ = short_tempdir_in(&deep);
    }
}
