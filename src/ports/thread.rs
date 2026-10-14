//! A comment on an issue's or pull request's thread, as the runner reads it

use super::Timestamp;

/// A comment kelpie posted as its App, as the forge numbered and dated it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostedComment {
    /// The forge's id for it
    pub id: u64,
    /// When the forge says it was written
    pub created: Timestamp,
}

/// A comment kelpie puts on one line of a pull request's diff, in a review
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineComment {
    /// The file it is on, as the diff names it
    pub path: String,
    /// The line in the file at the head, which the diff must show
    pub line: u32,
    /// What it says
    pub body: String,
}

/// A thumbs up or down someone left on a comment
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    /// Who left it
    pub login: String,
    /// Whether it is a thumbs up, and not a thumbs down
    pub up: bool,
}

/// What people did to a comment of kelpie's, short of replying
///
/// A ruling's comment is answered by a reaction on it or a box ticked in it.
/// Anyone with write access can edit a comment, so a tick counts only when
/// the forge says whose edit it was.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentMarks {
    /// The login that wrote the comment
    pub author: String,
    /// What it says now
    pub body: String,
    /// The thumbs up and down on it, other reactions left out
    pub thumbs: Vec<Thumb>,
    /// The login that made each edit of it, one entry an edit
    pub editors: Vec<String>,
}

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
