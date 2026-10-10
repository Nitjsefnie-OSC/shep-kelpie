//! The macOS keychain, asked with the system's `security`

use std::io::ErrorKind;
use std::process::Stdio;

use crate::github::Keychain;

/// `security find-generic-password -s gh:github.com`, which `gh` keeps its
/// login under. No `-w` or `-g`, so the token is never printed or read, and
/// its output is dropped.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecurityKeychain;

impl Keychain for SecurityKeychain {
    fn holds_gh_login(&self) -> Result<bool, String> {
        let status = crate::spawn::command("security")
            .args(["find-generic-password", "-s", "gh:github.com"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match status {
            Ok(status) => held(status.code()),
            // No `security`, so no keychain for `gh` to keep a login in.
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!("cannot run `security`: {e}")),
        }
    }
}

// `security` exits 0 for an entry found and 44 for none, as measured on
// macOS 26 (`errSecItemNotFound`). Any other exit, such as a locked keychain
// that cannot be asked without a person, says nothing either way.
fn held(code: Option<i32>) -> Result<bool, String> {
    match code {
        Some(0) => Ok(true),
        Some(44) => Ok(false),
        Some(other) => Err(format!("`security` exited {other}")),
        None => Err("`security` was ended by a signal".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entry_found_or_not_is_told_and_any_other_exit_is_not_a_no() {
        assert_eq!(held(Some(0)), Ok(true));
        assert_eq!(held(Some(44)), Ok(false));
        assert_eq!(held(Some(36)), Err("`security` exited 36".to_owned()));
        assert!(held(None).is_err());
    }
}
