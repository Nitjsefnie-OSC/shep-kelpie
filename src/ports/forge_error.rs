//! Why a forge call failed

use std::fmt;

use super::{Timestamp, rate_limit};
use crate::local_paths::Leak;

/// Why a forge call failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeError {
    /// The forge's command line tool could not be started, with the OS's reason
    Spawn(String),
    /// The tool ran and exited unsuccessfully, with this on stderr
    Failed(String),
    /// The tool succeeded but its output was not what was asked for
    Unreadable(String),
    /// A field of the post names something private to this machine, so kelpie never sent it
    LocalPath {
        /// The field, such as "the comment"
        what: &'static str,
        /// What it names
        leak: Leak,
    },
    /// The forge said its rate limit was used up, so no call is made until it resets then
    Held(Timestamp),
    /// The GitHub App's write failed, with why
    App(String),
    /// The GitHub App could not be asked just now, such as with GitHub out of
    /// reach, busy or rate limited, with why: asking again later may work
    AppBusy(String),
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "cannot run gh: {error}"),
            Self::Failed(stderr) => write!(f, "gh failed: {}", stderr.trim()),
            Self::Unreadable(output) => write!(f, "unreadable gh output: {}", output.trim()),
            Self::LocalPath { what, leak } => write!(f, "not posted: {what} names {leak}"),
            Self::Held(until) => write!(
                f,
                "not asked: the forge's rate limit is used up until {}",
                rate_limit::time_of(*until)
            ),
            Self::App(why) => write!(f, "the GitHub App could not post: {why}"),
            Self::AppBusy(why) => write!(f, "the GitHub App is not answering: {why}"),
        }
    }
}

impl core::error::Error for ForgeError {}
