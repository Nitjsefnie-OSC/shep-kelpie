//! The macOS keychain, asked with the system's `security`

use std::io::ErrorKind;
use std::process::Stdio;

use crate::github::Keychain;

/// `security find-generic-password -s gh:github.com`, which `gh` keeps its
/// login under, and `security find-internet-password -s github.com`, which
/// git's osxkeychain helper keeps the login it was given under (Apple's git
/// turns the helper on, so a maintainer who pushed over https has one). No
/// `-w` or `-g`, so a token is never printed or read, and the output is dropped.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecurityKeychain;

impl Keychain for SecurityKeychain {
    fn holds_github_login(&self) -> Result<bool, String> {
        either(
            ask(["find-generic-password", "-s", "gh:github.com"]),
            ask(["find-internet-password", "-s", "github.com"]),
        )
    }
}

fn ask(args: [&str; 3]) -> Result<bool, String> {
    let status = crate::spawn::command("security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) => held(status.code()),
        // No `security`, so no keychain for a login to be kept in.
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot run `security`: {e}")),
    }
}

// An entry of either kind is a login; one that could not be asked about is
// no answer, unless the other found one.
fn either(first: Result<bool, String>, second: Result<bool, String>) -> Result<bool, String> {
    match (first, second) {
        (Ok(true), _) | (_, Ok(true)) => Ok(true),
        (Ok(false), Ok(false)) => Ok(false),
        (Err(why), _) | (_, Err(why)) => Err(why),
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
    fn an_entry_of_either_kind_is_a_login_and_a_keychain_that_cannot_be_asked_is_no_answer() {
        let (yes, no, err) = (Ok(true), Ok(false), Err("locked".to_owned()));
        assert_eq!(either(no.clone(), yes.clone()), Ok(true));
        assert_eq!(either(err.clone(), yes), Ok(true));
        assert_eq!(either(no.clone(), no.clone()), Ok(false));
        assert_eq!(either(no, err.clone()), err);
    }

    #[test]
    fn an_entry_found_or_not_is_told_and_any_other_exit_is_not_a_no() {
        assert_eq!(held(Some(0)), Ok(true));
        assert_eq!(held(Some(44)), Ok(false));
        assert_eq!(held(Some(36)), Err("`security` exited 36".to_owned()));
        assert!(held(None).is_err());
    }
}
