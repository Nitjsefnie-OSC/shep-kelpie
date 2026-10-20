//! The token a call acts as the GitHub App with

use std::fmt;

/// An installation token, which acts as the App on the repos it is installed on
///
/// `Debug` does not show it.
#[derive(Clone, PartialEq, Eq)]
pub struct InstallationToken(String);

impl InstallationToken {
    /// The token `text`
    pub fn new(text: String) -> Self {
        Self(text)
    }

    /// The token's text, for the call that sends it
    #[inline]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for InstallationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstallationToken(..)")
    }
}
