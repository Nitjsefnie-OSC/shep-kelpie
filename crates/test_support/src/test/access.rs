//! Opening a file or folder to others, for a test of the check that refuses one

use std::path::Path;

/// Lets every user read `path`: group and other mode bits on Unix, an ACE for
/// Everyone on Windows, which leaves the owner and the rest of the ACL as
/// they were
///
/// # Panics
///
/// When the mode cannot be set or `icacls` fails.
#[track_caller]
pub fn open_to_others(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(windows)]
    {
        let grant = if path.is_dir() {
            "*S-1-1-0:(OI)(CI)R"
        } else {
            "*S-1-1-0:R"
        };
        grant_with_icacls(path, grant);
    }
}

/// Grants the Administrators group read access to `path`, which a file only
/// its owner may read does not allow
///
/// # Panics
///
/// When `icacls` fails.
#[cfg(windows)]
#[track_caller]
pub fn open_to_administrators(path: &Path) {
    grant_with_icacls(path, "*S-1-5-32-544:R");
}

#[cfg(windows)]
#[track_caller]
fn grant_with_icacls(path: &Path, grant: &str) {
    let granted = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant", grant])
        .output()
        .unwrap();
    assert!(granted.status.success(), "{granted:?}");
}
