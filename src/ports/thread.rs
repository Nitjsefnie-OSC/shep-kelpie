//! A comment on an issue's or pull request's thread, as the runner reads it

use super::Timestamp;

/// One comment on the thread of an issue or a pull request
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    /// The forge's id for it, which rises with each comment on the repo
    pub id: u64,
    /// The issue or pull request it is on
    pub thread: u64,
    /// Its author's login
    pub author: String,
    /// Whether its author is a bot or an App, which never answers a ruling
    pub bot: bool,
    /// What it says, as written
    pub body: String,
    /// When it was written
    pub created: Timestamp,
}
