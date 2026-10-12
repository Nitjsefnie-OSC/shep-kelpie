//! Getting a plugin marketplace onto this machine

use std::path::Path;

/// Clones a plugin marketplace's git repo into kelpie's home, or fetches it
/// if it is already there
///
/// The runner's own git does it, outside any call's sandbox, as it starts
/// and when it rereads its settings. A marketplace in a folder on this
/// machine is read where it is and never comes here.
pub trait Marketplaces: Send + Sync {
    /// Makes `into` hold the repo at `url`, at its default branch's tip
    ///
    /// # Errors
    ///
    /// Why git could not clone or fetch. Whatever `into` held before stays.
    fn fetch(&self, url: &str, into: &Path) -> Result<(), String>;
}
