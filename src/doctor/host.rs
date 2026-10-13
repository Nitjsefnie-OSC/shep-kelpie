//! What doctor asks of the machine itself, beyond the runner's ports

use std::fmt;

/// The machine kelpie runs on
pub trait Host: Send + Sync + fmt::Debug {
    /// The programs the sandbox runtime needs that are not on this machine,
    /// none when it can run
    fn sandbox_gaps(&self) -> Vec<&'static str>;

    /// Whether this machine keeps logins in a keychain a sandboxed command can ask
    fn has_keychain(&self) -> bool;

    /// This machine's operating system, as `std::env::consts::OS` names it
    fn os(&self) -> &'static str;

    /// Whether this shell's environment holds the login token `claude setup-token` makes
    fn login_token(&self) -> bool;

    /// Whether `program` is an executable file in a folder of `PATH`
    fn on_path(&self, program: &str) -> bool;

    /// The first line `program --version` prints, given ten seconds to print it
    ///
    /// # Errors
    ///
    /// A message when `program` does not run, does not answer in time, exits
    /// non-zero, or prints nothing.
    fn program_version(&self, program: &str) -> Result<String, String>;
}

/// Whether `found` is `minimum` or later, comparing major, minor and patch as numbers
///
/// None when either does not read as three dot-separated numbers, which a
/// pre-release such as `1.0.4-beta.1` does not.
pub fn at_least(found: &str, minimum: &str) -> Option<bool> {
    Some(numbers(found)? >= numbers(minimum)?)
}

fn numbers(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.');
    let mut number = || {
        let part = parts.next()?;
        // `parse` alone would take a leading `+`.
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let numbers = [number()?, number()?, number()?];
    parts.next().is_none().then_some(numbers)
}

/// The programs the sandbox runtime needs on `os`, as `std::env::consts::OS` names it
pub const fn sandbox_needs(os: &str) -> &'static [&'static str] {
    match os.as_bytes() {
        b"macos" => &["sandbox-exec"],
        b"linux" => &["bwrap", "socat"],
        _ => &[],
    }
}

/// The programs in `needs` that `on_path` does not find
pub fn gaps(needs: &[&'static str], on_path: impl Fn(&str) -> bool) -> Vec<&'static str> {
    needs
        .iter()
        .copied()
        .filter(|program| !on_path(program))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_needs_its_own_sandbox_and_linux_needs_two_programs() {
        assert_eq!(sandbox_needs("macos"), ["sandbox-exec"]);
        assert_eq!(sandbox_needs("linux"), ["bwrap", "socat"]);
        assert!(sandbox_needs("windows").is_empty());
    }

    #[test]
    fn a_version_is_compared_by_its_numbers_not_its_text() {
        for under in ["1.0.4", "0.99.2", "0.85.1", "1.0.3", "0.100.0"] {
            assert_eq!(at_least("1.0.10", under), Some(true), "1.0.10 over {under}");
            assert_eq!(
                at_least(under, "1.0.10"),
                Some(false),
                "{under} under 1.0.10"
            );
        }
        assert_eq!(at_least("1.0.4", "1.0.4"), Some(true));
        for text in [
            "1.0.4-beta.1",
            "1.0",
            "1.0.4.1",
            "v1.0.4",
            "",
            "latest",
            "1.0.x",
            "1.+0.4",
        ] {
            assert_eq!(at_least(text, "1.0.4"), None, "{text:?} found");
            assert_eq!(at_least("1.0.4", text), None, "{text:?} minimum");
        }
    }

    #[test]
    fn a_gap_is_a_needed_program_off_the_path() {
        let needs = sandbox_needs("linux");
        assert_eq!(gaps(needs, |_| true), Vec::<&str>::new());
        assert_eq!(gaps(needs, |p| p == "bwrap"), ["socat"]);
        assert_eq!(gaps(needs, |_| false), ["bwrap", "socat"]);
    }
}
