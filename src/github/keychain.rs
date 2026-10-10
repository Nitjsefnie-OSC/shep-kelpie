//! Whether the maintainer's GitHub login is in the macOS keychain, where `gh`
//! keeps it, and where git's osxkeychain helper keeps the one it was given
//!
//! A worker's sandbox lets a process ask the keychain (measured from a
//! worker's own session), and no file rule stops it, since securityd reads
//! the files. So on a repo the App covers, no worker starts while the login
//! is there: the maintainer moves it into `gh`'s `hosts.yml`, which the worker's
//! fence may not read.

/// The keychain the maintainer's `gh` login may be kept in
pub trait Keychain: Send + Sync {
    /// Whether a GitHub login is there, `gh`'s or git's, asked from outside
    /// any worker's sandbox
    ///
    /// A machine with no keychain holds none.
    ///
    /// # Errors
    ///
    /// Why the keychain could not be asked, such as a locked one: a worker is
    /// then not started, as with a login that is there.
    fn holds_github_login(&self) -> Result<bool, String>;
}
